/*
 * Copyright 2021 Oxide Computer Company
 * Copyright 2022 Till Wegmueller
 */

use anyhow::{anyhow, bail, Result};
use digest::Digest;
use log::{info, warn};
use std::ffi::CString;
use std::fs::{DirBuilder, File};
use std::io::{BufReader, Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub enum HashType {
    SHA1,
    MD5,
    None,
}

#[derive(Debug, PartialEq)]
pub enum FileType {
    Directory,
    File,
    Link,
}

#[derive(Debug, PartialEq)]
pub enum Id {
    #[allow(dead_code)]
    Name(String),
    Id(u32),
}

#[derive(Debug, PartialEq)]
pub struct FileInfo {
    pub filetype: FileType,
    pub perms: u32,
    pub owner: Id,
    pub group: Id,
    pub target: Option<PathBuf>, /* for symbolic links */
}

impl FileInfo {
    #[allow(dead_code)]
    pub fn is_user_executable(&self) -> bool {
        (self.perms & libc::S_IXUSR) != 0
    }

    pub fn is_file(&self) -> bool {
        matches!(&self.filetype, FileType::File)
    }
}

pub fn check<P: AsRef<Path>>(p: P) -> Result<Option<FileInfo>> {
    let name: &str = p.as_ref().to_str().unwrap();
    let cname = CString::new(name.to_string())?;
    let st = Box::into_raw(Box::new(unsafe { std::mem::zeroed::<libc::stat>() }));
    let (r, e, st) = unsafe {
        let r = libc::lstat(cname.as_ptr(), st);
        let e = *libc::___errno();
        (r, e, Box::from_raw(st))
    };
    if r != 0 {
        if e == libc::ENOENT {
            return Ok(None);
        }

        bail!("lstat({}): errno {}", name, e);
    }

    let fmt = st.st_mode & libc::S_IFMT;

    let filetype = if fmt == libc::S_IFDIR {
        FileType::Directory
    } else if fmt == libc::S_IFREG {
        FileType::File
    } else if fmt == libc::S_IFLNK {
        FileType::Link
    } else {
        bail!("lstat({}): unexpected file type: {:x}", name, fmt);
    };

    let target = if filetype == FileType::Link {
        Some(std::fs::read_link(p)?)
    } else {
        None
    };

    let owner = Id::Id(st.st_uid);
    let group = Id::Id(st.st_gid);

    let perms = st.st_mode & 0o7777; /* as per mknod(2) */

    Ok(Some(FileInfo {
        filetype,
        perms,
        owner,
        group,
        target,
    }))
}

pub fn chown<P: AsRef<Path>>(path: P, owner: u32, group: u32) -> Result<()> {
    let cname = CString::new(path.as_ref().to_str().unwrap().to_string())?;
    let (r, e) = unsafe {
        let r = libc::lchown(cname.as_ptr(), owner, group);
        let e = *libc::___errno();
        (r, e)
    };
    if r != 0 {
        bail!(
            "lchown({}, {}, {}): errno {}",
            path.as_ref().display(),
            owner,
            group,
            e
        );
    }

    Ok(())
}

pub fn perms<P: AsRef<Path>>(p: P, owner: u32, group: u32, perms: u32) -> Result<bool> {
    let p = p.as_ref();
    let mut did_work = false;

    let fi = if let Some(fi) = check(p)? {
        fi
    } else {
        bail!("{} does not exist", p.display());
    };

    /*
     * Check permissions and ownership on the path.  Note that symbolic links do
     * not actually have permissions, so we skip those completely.
     */
    if fi.filetype != FileType::Link && fi.perms != perms {
        did_work = true;
        info!("perms are {:o}, should be {:o}", fi.perms, perms);

        let cname = CString::new(p.to_str().unwrap().to_string())?;
        let (r, e) = unsafe {
            let r = libc::chmod(cname.as_ptr(), perms);
            let e = *libc::___errno();
            (r, e)
        };
        if r != 0 {
            bail!("lchmod({}, {:o}): errno {}", p.display(), perms, e);
        }

        info!("chmod ok");
    }

    match (fi.owner, fi.group) {
        (Id::Id(o), Id::Id(g)) if o == owner && g == group => {
            info!("ownership already OK ({}:{})", o, g);
        }
        (o, g) => {
            did_work = true;
            info!("ownership wrong ({:?}:{:?}, not {}:{})", o, g, owner, group);
            chown(p, owner, group)?;

            info!("chown ok");
        }
    };

    Ok(did_work)
}

pub fn directory<P: AsRef<Path>>(dir: P, owner: u32, group: u32, mode: u32) -> Result<bool> {
    let dir = dir.as_ref();
    let mut did_work = false;

    if let Some(fi) = check(dir)? {
        /*
         * The path exists already.  Make sure it is a directory.
         */
        if fi.filetype != FileType::Directory {
            bail!("{} is {:?}, not a directory", dir.display(), fi.filetype);
        }
    } else {
        /*
         * Create the directory, and all missing parents:
         */
        did_work = true;
        info!("creating directory: {}", dir.display());
        DirBuilder::new().recursive(true).mode(mode).create(dir)?;

        /*
         * Check the path again, to make sure we have up-to-date information:
         */
        check(dir)?.expect("directory should now exist");
    }

    if perms(dir, owner, group, mode)? {
        did_work = true;
    }

    Ok(did_work)
}

#[derive(Debug, PartialEq)]
#[allow(dead_code)]
pub enum Create {
    IfMissing,
    Always,
}

fn open<P: AsRef<Path>>(p: P) -> Result<File> {
    let p = p.as_ref();

    match File::open(p) {
        Ok(f) => Ok(f),
        Err(e) => Err(anyhow!("opening \"{}\": {}", p.display(), e)),
    }
}

fn comparestr<P: AsRef<Path>>(src: &str, dst: P) -> Result<bool> {
    let dstf = open(dst)?;
    let mut dstr = BufReader::new(dstf);

    /*
     * Assume that if the file can be passed in as a string slice, it can also
     * be loaded into memory fully for comparison.
     */
    let mut dstbuf = Vec::<u8>::new();
    dstr.read_to_end(&mut dstbuf)?;

    Ok(dstbuf == src.as_bytes())
}

fn compare<P1: AsRef<Path>, P2: AsRef<Path>>(src: P1, dst: P2) -> Result<bool> {
    let srcf = open(src)?;
    let dstf = open(dst)?;
    let mut srcr = BufReader::new(srcf);
    let mut dstr = BufReader::new(dstf);

    loop {
        let mut srcbuf = [0u8; 1];
        let mut dstbuf = [0u8; 1];
        let srcsz = srcr.read(&mut srcbuf)?;
        let dstsz = dstr.read(&mut dstbuf)?;

        if srcsz != dstsz {
            /*
             * Files are not the same size...
             */
            return Ok(false);
        }

        if srcsz == 0 {
            /*
             * End-of-file reached, without a mismatched comparison.  These
             * files are equal in contents.
             */
            return Ok(true);
        }

        if srcbuf != dstbuf {
            /*
             * This portion of the read files are not the same.
             */
            return Ok(false);
        }
    }
}

pub fn removed<P: AsRef<Path>>(dst: P) -> Result<()> {
    let dst = dst.as_ref();

    if let Some(fi) = check(dst)? {
        match fi.filetype {
            FileType::File | FileType::Link => {
                info!(
                    "file {} exists (as {:?}), removing",
                    dst.display(),
                    fi.filetype
                );

                std::fs::remove_file(dst)?;
            }
            t => {
                bail!("file {} exists as {:?}, unexpected type", dst.display(), t);
            }
        }
    } else {
        info!(
            "file {} does not already exist, skipping removal",
            dst.display()
        );
    }

    Ok(())
}

pub fn filestr<P: AsRef<Path>>(
    contents: &str,
    dst: P,
    owner: u32,
    group: u32,
    mode: u32,
    create: Create,
) -> Result<bool> {
    let dst = dst.as_ref();
    let mut did_work = false;

    let do_copy = if let Some(fi) = check(dst)? {
        /*
         * The path exists already.
         */
        match create {
            Create::IfMissing if fi.filetype == FileType::File => {
                info!("file {} exists, skipping population", dst.display());
                false
            }
            Create::IfMissing if fi.filetype == FileType::Link => {
                warn!("symlink {} exists, skipping population", dst.display());
                false
            }
            Create::IfMissing => {
                /*
                 * Avoid clobbering an unexpected entry when we have been asked
                 * to preserve in the face of modifications.
                 */
                bail!(
                    "{} should be a file, but is a {:?}",
                    dst.display(),
                    fi.filetype
                );
            }
            Create::Always if fi.filetype == FileType::File => {
                /*
                 * Check the contents of the file to make sure it matches
                 * what we expect.
                 */
                if comparestr(contents, dst)? {
                    info!("file {} exists, with correct contents", dst.display());
                    false
                } else {
                    warn!(
                        "file {} exists, with wrong contents, unlinking",
                        dst.display()
                    );
                    std::fs::remove_file(dst)?;
                    true
                }
            }
            Create::Always => {
                /*
                 * We found a file type we don't expect.  Try to unlink it
                 * anyway.
                 */
                warn!(
                    "file {} exists, of type {:?}, unlinking",
                    dst.display(),
                    fi.filetype
                );
                std::fs::remove_file(dst)?;
                true
            }
        }
    } else {
        info!("file {} does not exist", dst.display());
        true
    };

    if do_copy {
        did_work = true;
        info!("writing {} ...", dst.display());

        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&dst)?;
        f.write_all(contents.as_bytes())?;
        f.flush()?;
    }

    if perms(dst, owner, group, mode)? {
        did_work = true;
    }

    info!("ok!");
    Ok(did_work)
}

pub fn file<P1: AsRef<Path>, P2: AsRef<Path>>(
    src: P1,
    dst: P2,
    owner: u32,
    group: u32,
    mode: u32,
    create: Create,
) -> Result<bool> {
    let src = src.as_ref();
    let dst = dst.as_ref();
    let mut did_work = false;

    let do_copy = if let Some(fi) = check(dst)? {
        /*
         * The path exists already.
         */
        match create {
            Create::IfMissing if fi.filetype == FileType::File => {
                info!("file {} exists, skipping population", dst.display());
                false
            }
            Create::IfMissing if fi.filetype == FileType::Link => {
                warn!("symlink {} exists, skipping population", dst.display());
                false
            }
            Create::IfMissing => {
                /*
                 * Avoid clobbering an unexpected entry when we have been asked
                 * to preserve in the face of modifications.
                 */
                bail!(
                    "{} should be a file, but is a {:?}",
                    dst.display(),
                    fi.filetype
                );
            }
            Create::Always if fi.filetype == FileType::File => {
                /*
                 * Check the contents of the file to make sure it matches
                 * what we expect.
                 */
                if compare(src, dst)? {
                    info!("file {} exists, with correct contents", dst.display());
                    false
                } else {
                    warn!(
                        "file {} exists, with wrong contents, unlinking",
                        dst.display()
                    );
                    std::fs::remove_file(dst)?;
                    true
                }
            }
            Create::Always => {
                /*
                 * We found a file type we don't expect.  Try to unlink it
                 * anyway.
                 */
                warn!(
                    "file {} exists, of type {:?}, unlinking",
                    dst.display(),
                    fi.filetype
                );
                std::fs::remove_file(dst)?;
                true
            }
        }
    } else {
        info!("file {} does not exist", dst.display());
        true
    };

    if do_copy {
        did_work = true;
        info!("copying {} -> {} ...", src.display(), dst.display());
        std::fs::copy(src, dst)?;
    }

    if perms(dst, owner, group, mode)? {
        did_work = true;
    }

    info!("ok!");
    Ok(did_work)
}

pub fn symlink<P1: AsRef<Path>, P2: AsRef<Path>>(
    dst: P1,
    target: P2,
    owner: u32,
    group: u32,
) -> Result<bool> {
    let dst = dst.as_ref();
    let target = target.as_ref();
    let mut did_work = false;

    let do_link = if let Some(fi) = check(dst)? {
        if fi.filetype == FileType::Link {
            let fitarget = fi.target.unwrap();
            if fitarget == target {
                info!("link target ok ({})", target.display());
                false
            } else {
                warn!(
                    "link target wrong: want {}, got {}; unlinking",
                    target.display(),
                    fitarget.display()
                );
                std::fs::remove_file(dst)?;
                true
            }
        } else {
            /*
             * File type not correct.  Unlink.
             */
            warn!(
                "file {} exists, of type {:?}, unlinking",
                dst.display(),
                fi.filetype
            );
            std::fs::remove_file(dst)?;
            true
        }
    } else {
        info!("link {} does not exist", dst.display());
        true
    };

    if do_link {
        did_work = true;
        info!("linking {} -> {} ...", dst.display(), target.display());
        std::os::unix::fs::symlink(target, dst)?;
    }

    if perms(dst, owner, group, 0)? {
        did_work = true;
    }

    info!("ok!");
    Ok(did_work)
}

pub fn hash_file<P: AsRef<Path>>(p: P, hash_type: &HashType) -> Result<String> {
    let p = p.as_ref();

    if let HashType::None = hash_type {
        return Ok("".to_string());
    }

    let f = File::open(p)?;
    let mut r = BufReader::new(f);
    let mut buf = [0u8; 128 * 1024];

    let mut digest: Box<dyn digest::DynDigest> = match hash_type {
        HashType::MD5 => Box::new(md5::Md5::new()),
        HashType::SHA1 => Box::new(sha1::Sha1::new()),
        HashType::None => panic!("None unexpected"),
    };

    loop {
        let sz = r.read(&mut buf)?;
        if sz == 0 {
            break;
        }

        digest.update(&buf[0..sz]);
    }

    let mut out = String::new();
    let hash = digest.finalize();
    for byt in hash.iter() {
        out.push_str(&format!("{:02x}", byt));
    }

    Ok(out)
}
