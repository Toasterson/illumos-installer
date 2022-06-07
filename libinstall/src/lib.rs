mod ensure;
mod keywords;
mod zfs;

use crate::keywords::get_supported_keywords;
use anyhow::{anyhow, bail, format_err, Context, Error, Result};
use libcfgparser::Keyword;
use log::{debug, info, trace};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::min;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::{fs, path, thread};
use thiserror::Error;
use uuid::Uuid;

static INSTALLER_TMP_DIR: &str = "/var/tmp/installer";

#[derive(Debug, Clone)]
enum ImageCompression {
    Gzip,
    Lz4,
    Zstd,
}

#[derive(Debug, Clone)]
enum ImageType {
    Tarball,
    ZfsStream,
}

#[derive(Debug, Clone)]
struct ImageInfo {
    image_type: ImageType,
    compression: ImageCompression,
    path: String,
}

#[derive(Debug, Default)]
struct ImageDownloadProgress {
    name: String,
    size: usize,
    downloaded: usize,
    percentage: f64,
}

impl Display for ImageDownloadProgress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}/{} {:.1}%",
            self.name, self.downloaded, self.size, self.percentage
        )
    }
}

pub type InstructionsSet = Vec<Instruction>;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case", tag = "t")]
pub enum Instruction {
    CreatePool {
        name: String,
        vdevs: Vec<VDEVConfiguration>,
        ashift: Option<i32>,
        uefi: bool,
        be_name: Option<String>,
        pool_options: Option<Vec<(String, String)>>,
    },
    CreateDataset {
        name: String,
        #[serde(flatten)]
        properties: HashMap<String, Value>,
    },
    InstallImage {
        src: String,
        pool: String,
    },
    Include {
        name: String,
    },
    MakeBootable {
        pool: String,
        be_name: String,
    },
    EnsureFile {
        src: Option<String>,
        image_src: Option<String>,
        contents: Option<String>,
        file: String,
        owner: String,
        group: String,
        mode: String,
    },
    TemplateFile {
        src: Option<String>,
        contents: Option<String>,
        file: String,
        owner: String,
        group: String,
        mode: String,
    },
    EnsureSymlink {
        link: String,
        target: String,
        owner: String,
        group: String,
    },
    EnsureDir {
        dir: String,
        owner: String,
        group: String,
        mode: String,
    },
    RemoveFiles {
        dir: String,
    },
    Devfsadm,
    Shadow {
        username: String,
        password: String,
    },
    AssembleFiles {
        dir: String,
        output: String,
        prefix: String,
    },
    PkgImageCreate {
        publisher: String,
        uri: String,
    },
    PkgInstall {
        pkgs: Vec<String>,
    },
    PkgUninstall {
        pkgs: Vec<String>,
    },
    PkgSetProperty {
        name: String,
        value: String,
    },
    PkgChangeVariant {
        variant: String,
        value: String,
    },
    PkgSetMediator {
        implementation: Option<String>,
        version: Option<String>,
        mediator: String,
    },
    PkgUnsetMediator {
        mediator: String,
    },
    PkgChangeFacet {
        facet: String,
        value: Option<bool>,
    },
    PkgSetPublisher {
        publisher: String,
        uri: String,
        mirror_uri: String,
        sticky: bool,
        search_first: bool,
    },
    PkgUnsetPublisher {
        publisher: String,
    },
    PkgPurgeHistory,
    PkgRebuildIndex,
    SeedSmf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VDEVType {
    Empty,
    Mirror,
    RaidZ1,
    RaidZ2,
    RaidZ3,
}

impl ToString for VDEVType {
    fn to_string(&self) -> String {
        match self {
            VDEVType::Empty => String::new(),
            VDEVType::Mirror => String::from("mirror"),
            VDEVType::RaidZ1 => String::from("raidz1"),
            VDEVType::RaidZ2 => String::from("raidz2"),
            VDEVType::RaidZ3 => String::from("raidz3"),
        }
    }
}

impl Default for VDEVType {
    fn default() -> Self {
        Self::Empty
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct VDEVConfiguration {
    pub vdev_type: VDEVType,
    pub devices: Vec<String>,
}

#[derive(Error, Debug)]
enum InstructionError {
    #[error("keyword {0} is not known")]
    UnknownInstruction(String),
}

pub fn parse_keywords(keywords: Vec<Keyword>) -> Result<InstructionsSet> {
    let mut set = InstructionsSet::new();
    for c in keywords {
        match c.name.as_str() {
            "zpool-create" => {
                let mut vdevs: Vec<VDEVConfiguration> = vec![];
                let pool_options = if let Some(opts) = c.options.clone() {
                    Some(
                        opts.into_iter()
                            .filter(|(k, _)| k != "ashift" || k != "uefi")
                            .map(|(k, v)| (k, v))
                            .collect::<Vec<(String, String)>>(),
                    )
                } else {
                    None
                };

                let (ashift, uefi, be_name) = if let Some(opts) = c.options {
                    (
                        if opts.contains_key("ashift") {
                            let ashift = opts["ashift"].clone();
                            Some(ashift.parse::<i32>().context("ashift is not an integer")?)
                        } else {
                            None
                        },
                        if opts.contains_key("uefi") {
                            true
                        } else {
                            false
                        },
                        if opts.contains_key("be_name") {
                            Some(opts["be_name"].clone())
                        } else {
                            None
                        },
                    )
                } else {
                    (None, true, None)
                };

                let mut name = String::new();
                let mut vdev_config = VDEVConfiguration::default();
                for (i, opt) in c.arguments.into_iter().enumerate() {
                    if i == 0 {
                        name = opt.clone();
                        continue;
                    }

                    match opt.as_str() {
                        "mirror" => {
                            if vdev_config.vdev_type != VDEVType::Empty {
                                vdevs.push(vdev_config.clone());
                                vdev_config = VDEVConfiguration::default();
                            }
                            vdev_config.vdev_type = VDEVType::Mirror
                        }
                        "raidz" | "raidz1" => {
                            if vdev_config.vdev_type != VDEVType::Empty {
                                vdevs.push(vdev_config.clone());
                                vdev_config = VDEVConfiguration::default();
                            }
                            vdev_config.vdev_type = VDEVType::RaidZ1
                        }
                        "raidz2" => {
                            if vdev_config.vdev_type != VDEVType::Empty {
                                vdevs.push(vdev_config.clone());
                                vdev_config = VDEVConfiguration::default();
                            }
                            vdev_config.vdev_type = VDEVType::RaidZ2
                        }
                        "raidz3" => {
                            if vdev_config.vdev_type != VDEVType::Empty {
                                vdevs.push(vdev_config.clone());
                                vdev_config = VDEVConfiguration::default();
                            }
                            vdev_config.vdev_type = VDEVType::RaidZ3
                        }
                        _ => vdev_config.devices.push(opt),
                    }
                }
                vdevs.push(vdev_config.clone());
                set.push(Instruction::CreatePool {
                    name,
                    vdevs,
                    ashift,
                    uefi,
                    be_name,
                    pool_options,
                });
            }
            "create_be" => {
                if c.arguments.len() == 1 {
                    set.push(Instruction::CreateBootEnvironment {
                        pool_name: c.arguments[0].clone(),
                        name: None,
                    });
                } else {
                    set.push(Instruction::CreateBootEnvironment {
                        pool_name: c.arguments[0].clone(),
                        name: Some(c.arguments[1].clone()),
                    });
                }
            }
            "image" | "install_image" => {
                let pool_name = if let Some(opts) = c.options {
                    if opts.contains_key("pool") {
                        opts["pool"].clone()
                    } else {
                        "rpool".into()
                    }
                } else {
                    "rpool".into()
                };
                set.push(Instruction::InstallImage {
                    src: c.arguments[0].clone(),
                    pool,
                });
            }
            "ds" | "dataset" => {
                let opts: HashMap<String, Value> = if let Some(opts) = c.options {
                    opts.into_iter()
                        .map(|(k, v)| (k, Value::String(v)))
                        .collect()
                } else {
                    HashMap::new()
                };
                set.push(Instruction::CreateDataset {
                    name: c.arguments[0].clone(),
                    properties: opts,
                });
            }
            _ => {
                return Err(anyhow!(InstructionError::UnknownInstruction(
                    c.name.clone()
                )))
            }
        }
    }

    Ok(set)
}

pub fn read_instructions_file<P: AsRef<Path>>(path: P) -> Result<InstructionsSet> {
    let path = path.as_ref();

    let mut parser = libcfgparser::SysConfigParser::default();
    for (key, v) in get_supported_keywords() {
        trace!(target: "libinstall", "Adding Keyword {} to parser", &key);
        parser.add_keyword(key, v);
    }

    debug!(target: "libinstall", "Parsing config file");
    if let Some(ext) = path.extension() {
        if ext == "json" {
            let f = File::open(path)?;
            debug!(target: "libinstall", "Parsing JSON config");
            let set: InstructionsSet = serde_json::from_reader(f)?;
            Ok(set)
        } else if ext == "yml" || ext == "yaml" {
            let f = File::open(path)?;
            debug!(target: "libinstall", "Parsing YAML config");
            let set: InstructionsSet = serde_yaml::from_reader(f)?;
            Ok(set)
        } else if ext == "ron" {
            let file_content = fs::read_to_string(path)?;
            debug!(target: "libinstall", "Parsing RON config");
            let set: InstructionsSet = ron::from_str(&file_content)?;
            Ok(set)
        } else {
            debug!(target: "libinstall", "Parsing Custom kickstart format");
            let keywords = parser.parse_config_file(path)?;
            parse_keywords(keywords)
        }
    } else {
        debug!(target: "libinstall", "Parsing Custom kickstart format");
        let keywords = parser.parse_config_file(path)?;
        parse_keywords(keywords)
    }
}

/*
 * Hard-coded user ID and group ID for root:
 */
const ROOT: u32 = 0;

/*
 * We cannot correctly use the name service switch to translate user IDs for use
 * in the target image, as the database within the target may not match the
 * build system.  For now, assume we only need to deal with a handful of
 * hard-coded user names.
 */
fn translate_uid(user: &str) -> Result<u32> {
    Ok(match user {
        "root" => ROOT,
        "daemon" => 1,
        "bin" => 2,
        "sys" => 3,
        "adm" => 4,
        n => bail!("unknown user \"{}\"", n),
    })
}

/*
 * The situation is the same for group IDs as it is for user IDs.  See comments
 * for translate_uid().
 */
fn translate_gid(group: &str) -> Result<u32> {
    Ok(match group {
        "root" => ROOT,
        "other" => 1,
        "bin" => 2,
        "sys" => 3,
        "adm" => 4,
        n => bail!("unknown group \"{}\"", n),
    })
}

fn installer_pool_name(name: &str) -> String {
    format!("INSTALLER-{}", name)
}

fn installer_altroot(name: &str) -> String {
    format!("/altroot-{}", name)
}

pub fn apply_instruction<P: AsRef<Path>>(bundle_path: P, instruction: Instruction) -> Result<()> {
    match instruction {
        Instruction::CreatePool {
            name,
            vdevs,
            ashift,
            uefi,
            be_name,
            pool_options,
        } => create_pool(&name, vdevs, ashift, uefi, be_name, pool_options),
        Instruction::CreateDataset { name, properties } => create_dataset(&name, properties),
        Instruction::InstallImage { src, pool } => install_image(&src, &pool),
        Instruction::Include { name } => include_file(&bundle_path, name),
        Instruction::MakeBootable { pool, be_name } => make_bootable(&pool, &be_name),
        Instruction::EnsureFile {
            src,
            image_src,
            contents,
            file,
            owner,
            group,
            mode,
        } => {}
        Instruction::TemplateFile { .. } => {}
        Instruction::EnsureSymlink { .. } => {}
        Instruction::EnsureDir { .. } => {}
        Instruction::RemoveFiles { .. } => {}
        Instruction::Devfsadm => {}
        Instruction::Shadow { .. } => {}
        Instruction::AssembleFiles { .. } => {}
        Instruction::PkgImageCreate { .. } => {}
        Instruction::PkgInstall { .. } => {}
        Instruction::PkgUninstall { .. } => {}
        Instruction::PkgSetProperty { .. } => {}
        Instruction::PkgChangeVariant { .. } => {}
        Instruction::PkgSetMediator { .. } => {}
        Instruction::PkgUnsetMediator { .. } => {}
        Instruction::PkgChangeFacet { .. } => {}
        Instruction::PkgSetPublisher { .. } => {}
        Instruction::PkgUnsetPublisher { .. } => {}
        Instruction::PkgPurgeHistory => {}
        Instruction::PkgRebuildIndex => {}
        Instruction::SeedSmf => {}
    }
}

fn make_bootable(pool: &str, be_name: &str) -> Result<(), Error> {
    let pool = pool.as_ref();
    let be_name = be_name.as_ref();

    let alt_root = installer_altroot(pool);
    let installer_pool_name = installer_pool_name(pool);

    let root_ds = format!("{}/ROOT", installer_pool_name);
    let beds = format!("{}/{}", root_ds, be_name);
    zfs::zpool_set(pool, "bootfs", &beds)?;

    ensure::run(&["/sbin/beadm", "activate", be_name])?;
    ensure::run(&[
        "/sbin/bootadm",
        "install-bootloader",
        "-M",
        "-f",
        "-P",
        &installer_pool_name,
        "-R",
        &alt_root,
    ])?;
    ensure::run(&["/sbin/bootadm", "update-archive", "-f", "-R", &alt_root])?;

    Ok(())
}

fn create_pool(
    name: &String,
    vdevs: Vec<VDEVConfiguration>,
    ashift: Option<i32>,
    uefi: bool,
    be_name: Option<String>,
    pool_options: Option<Vec<(String, String)>>,
) -> Result<()> {
    /*
     * Create the new pool, using the temporary pool name while it is imported
     * on this system.  We specify an altroot to avoid using the system cache
     * file, and to avoid mountpoint clashes with the system pool.  If we do not
     * explicitly set the mountpoint of the pool (create -m ...) then it will
     * default to the dynamically constructed "/$poolname", which will be
     * correct both on this system and on the target system when it is
     * eventually imported as its target name.
     */
    let mut args = vec![
        "/sbin/zpool",
        "create",
        "-d",
        "-t",
        &installer_pool_name(&name),
        "-O",
        "compression=on",
        "-R",
        &installer_altroot(&name),
    ];

    if uefi {
        /*
         * If we need UEFI support, we must pass -B to create the
         * ESP slice.  Note that this consumes 256MB of space in the
         * image.
         */
        args.push("-B");
    }

    if let Some(ashift) = ashift {
        args.push("-o");
        let ashiftarg = format!("ashift={}", ashift);
        args.push(&ashiftarg);
    }

    args.push(&name);

    if let Some(pool_options) = pool_options {
        for (key, value) in pool_options {
            args.push("-o");
            let opt = format!("{}={}", key, value);
            args.push(&opt);
        }
    }

    let mut single_disk_added = false;
    for vdev in vdevs {
        match vdev.vdev_type {
            VDEVType::Empty => {
                if vdev.devices.len() != 1 || single_disk_added {
                    bail!("more than one single disk defined in pool {} bailing", name)
                }
                args.push(&vdev.devices[0]);
                single_disk_added = true;
            }
            _ => {
                args.push(&vdev.vdev_type.to_string());
                for dev in vdev.devices {
                    args.push(&dev);
                }
            }
        }
    }

    illumos::run(args.as_slice(), None)?;

    let be_name = create_be(pool_name, be_name)?;

    make_bootable(pool_name, &be_name)
}

fn create_dataset(name: &str, properties: HashMap<String, Value>) -> Result<()> {
    let props = properties
        .into_iter()
        .map(|(k, v)| (k, v.to_string()))
        .collect::<Vec<String, String>>();
    zfs::dataset_create(name, true, &props)
}

fn install_image(src: &String, pool: &String) -> Result<(), Error> {
    let (tx, rx) = channel::<ImageDownloadProgress>();

    let client = Client::new();

    let file_name = url.rsplitn(1, '/').collect::<Vec<String>>()[0].clone();
    let tmp_path = Path::new(INSTALLER_TMP_DIR)
        .join("download")
        .join(&file_name);

    ensure::directory(
        Path::new(INSTALLER_TMP_DIR).join("download"),
        ROOT,
        ROOT,
        0o755,
    )?;

    let sender = thread::spawn(move || download_file(&client, &src, &tmp_path, tx));

    let receiver = thread::spawn(move || {
        let value = rx.recv().expect("Unable to receive from channel");
        info!(value);
    });

    sender.join().expect("The sender thread has panicked");
    receiver.join().expect("The receiver thread has panicked");

    // Make sure we have a mounted boot environment
    let extract_dir = installer_altroot(&pool);
    ensure::check(&extract_dir)?;

    ensure::run(
        log,
        &[
            "/usr/sbin/tar",
            "xzeEp@/f",
            tmp_path.to_str().ok_or_else(anyhow!(
                "temporary path of downloaded tar file has non parseable characters in its name"
            ))?,
            "-C",
            &extract_dir,
        ],
    )?;

    Ok(())
}

fn include_file<P: AsRef<Path>>(bundle_path: &P, name: String) {
    let file_name = bundle_path.as_ref().join(name);
    let instructions = read_instructions_file(file_name)?;

    for instruction in instructions {
        apply_instruction(&bundle_path, instruction)?;
    }
}

async fn download_file<T, P: AsRef<Path>>(
    client: &Client,
    url: &str,
    path: P,
    tx: Sender<T>,
) -> Result<(), String> {
    // Info setup
    let mut info = ImageDownloadProgress::default();
    let path = path.as_ref();

    // Reqwest setup
    let res = client
        .get(url)
        .send()
        .await
        .or(Err(format!("Failed to GET from '{}'", &url)))?;
    info.size = res
        .content_length()
        .ok_or(format!("Failed to get content length from '{}'", &url))? as usize;

    info.name = res.url().path().rsplitn(1, '/').collect::<Vec<String>>()[0].clone();

    // download chunks
    let mut file =
        File::create(path).or(Err(format!("Failed to create file '{}'", path.display())))?;
    let mut stream = res.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item.or(Err(anyhow!("failed to download chunk of {}", url)))?;
        file.write_all(&chunk).or(Err(anyhow!(
            "Could not write to tmp file {}",
            path.display()
        )))?;
        info.downloaded = min(info.downloaded + (chunk.len()), total_size);
        tx.send(&info)?;
    }

    Ok(())
}

fn create_be(pool_name: String, name: Option<String>) -> Result<String, Error> {
    /*
     * Create be root:
     */
    let root_ds = format!("{}/ROOT", pool_name);
    let root_ds_props = [("canmount", "off"), ("mountpoint", "legacy")];
    zfs::dataset_create(&root_ds, false, &root_ds_props)?;

    /*
     * Create a BE of sorts:
     */
    let be_name = if let Some(name) = name {
        name
    } else {
        /*
         * XXX Generate a unique bename.  This is presently necessary because beadm
         * does not accept an altroot (-R) flag, and thus the namespace for boot
         * environments overlaps between "real" boot environments in use on the host
         * and any we create on the target image while it is mounted.
         *
         * Ideally, this will go away with changes to illumos.
         */
        Uuid::new_v4().to_hyphenated().to_string()[0..8].to_string()
    };
    let beds = format!("{}/{}", root_ds, be_name);
    let beds_props = [("canmount", "noauto"), ("mountpoint", "legacy")];
    zfs::dataset_create(&beds, false, &beds_props)?;

    /*
     * Mount that BE:
     */
    ensure::directory("/a", ROOT, ROOT, 0o755)?;
    illumos::run(&["/sbin/mount", "-F", "zfs", &beds, "/a"], None)?;

    /*
     * Set some BE properties...
     */
    let uuid = Uuid::new_v4().to_hyphenated().to_string();
    info!("boot environment UUID: {}", uuid);
    zfs::zfs_set(&beds, "org.opensolaris.libbe:uuid", &uuid)?;
    zfs::zfs_set(&beds, "org.opensolaris.libbe:policy", "static")?;

    Ok(be_name)
}

#[cfg(test)]
mod tests {
    use crate::Instruction::{Devfsadm, InstallImage};
    use crate::{Instruction, InstructionsSet, VDEVConfiguration, VDEVType};
    use serde_json::Value;
    use std::collections::HashMap;

    #[test]
    fn serialisation_test() {
        let config_ast: InstructionsSet = vec![
            Instruction::CreatePool {
                name: "rpool".into(),
                vdevs: vec![VDEVConfiguration {
                    vdev_type: VDEVType::Mirror,
                    devices: vec!["c1t0d0s0".into(), "c2t0d0s0".into(), "c3t0d0s0".into()],
                }],
                ashift: Some(12),
                uefi: true,
                be_name: None,
                pool_options: Some(vec![("blub".into(), "12".into())]),
            },
            Instruction::CreateDataset {
                name: "rpool/test".to_string(),
                properties: HashMap::from([(
                    "mountpoint".to_string(),
                    Value::String("legacy".to_string()),
                )]),
            },
            InstallImage {
                src: "https://dlc.openindiana.org/latest/openindiana_minimal.tar.gz".into(),
                pool: "rpool".into(),
            },
            Devfsadm,
        ];
        let serialized = serde_json::to_string_pretty(&config_ast).unwrap();
        println!("serialized = {}", serialized);

        let deserialized: InstructionsSet = serde_json::from_str(&serialized).unwrap();
        println!("deserialized = {:?}", deserialized);
    }
}
