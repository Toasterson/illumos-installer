use std::fs;
use std::fs::File;
use std::path::PathBuf;
use anyhow::{Result};
use clap::{Parser};
use std::process::Command as PCommand;
use libsysconfig::InstructionsSet;

static SMF_CONFIG_FILE_PROPERTY: &str = "config/file";
static SMF_FINISHED_PROPERTY: &str = "config/finished";

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    // File that holds the system config to apply
    #[clap(long, default_value="/etc/sysconfig.json")]
    file: PathBuf,

    // SMF FMRI is an environment variable that we need. it is set by SMF by default
    #[clap(env)]
    smf_fmri: Option<String>,

    // Alternate root
    #[clap(short='R', long)]
    alt_root: Option<String>,
}

fn main() -> Result<()> {
    let cli: Cli = Cli::parse();

    if let Some(smf_fmri) = cli.smf_fmri.clone() {
        // Check if we have run before and exit if we did
        let cfg_finished_prop = libsysconfig::svcprop(SMF_FINISHED_PROPERTY, &smf_fmri)?;
        if let Some(finished) = cfg_finished_prop {
            if finished == String::from("true") {
                return Ok(())
            }
        }
    }

    let cfg_file_prop = if let Some(smf_fmri) = cli.smf_fmri.clone() {
        libsysconfig::svcprop(SMF_CONFIG_FILE_PROPERTY, &smf_fmri)?
    } else {
        None
    };

    let cfg_file = if let Some(p) = cfg_file_prop {
        PathBuf::from(p)
    } else {
        cli.file
    };

    let mut parser = libcfgparser::SysConfigParser::default();
    for (key, v) in libsysconfig::get_supported_keywords() {
        parser.add_keyword(key, v);
    }

    let instructions = if let Some(ext) = cfg_file.extension() {
        if ext == "json" {
            let f = File::open(cfg_file)?;
            let set: InstructionsSet = serde_json::from_reader(f)?;
            set
        } else if ext == "yml" || ext == "yaml" {
            let f = File::open(cfg_file)?;
            let set: InstructionsSet = serde_yaml::from_reader(f)?;
            set
        } else if ext == "ron" {
            let file_content = fs::read_to_string(cfg_file)?;
            let set: InstructionsSet = ron::from_str(&file_content)?;
            set
        } else {
            let keywords = parser.parse_config_file(cfg_file)?;
            libsysconfig::parse_keywords(keywords)?
        }
    } else {
        let keywords = parser.parse_config_file(cfg_file)?;
        libsysconfig::parse_keywords(keywords)?
    };

    // If we are nor running under SMF require an alternate root
    let img = if cli.smf_fmri == None {
        if let Some(alt_root) = cli.alt_root {
            libsysconfig::Image::new(&alt_root)
        } else {
            // If we are not running under illumos SMF use a mocking driver
            libsysconfig::Image::new_with_driver("/", libsysconfig::Driver::Mock)
        }
    } else {
        libsysconfig::Image::new("/")
    };

    // Apply configuration
    for instruction in instructions {
        dbg!(img.apply_instruction(instruction)?);
    }

    // If we run under SMF setup run blocker so we don't run a second time
    if let Some(smf_fmri) = cli.smf_fmri.clone() {
        // rm -f /etc/.UNCONFIGURED
        let mut rm_cmd = PCommand::new("/usr/bin/rm");
        rm_cmd.arg("-f").arg("/etc/.UNCONFIGURED");

        // svccfg -s ${SMF_FMRI} "setprop config/finished=true"
        let finished_set_str = format!("{}=true", SMF_FINISHED_PROPERTY);
        libsysconfig::svccfg("/", vec!["-s", &smf_fmri, "setprop", &finished_set_str])?;

        // svccfg -s ${SMF_FMRI} "refresh"
        libsysconfig::svccfg("/", vec!["-s", &smf_fmri, "refresh"])?;
    }

    Ok(())
}
