use anyhow::Result;
use clap::Parser;
use libsysconfig::InstructionsSet;
use log::{debug, info, trace};
use slog::{Drain, Logger};
use slog_async::Async;
use slog_scope::{set_global_logger, GlobalLoggerGuard};
use slog_syslog::Facility;
use slog_term::{CompactFormat, TermDecorator};
use std::fs;
use std::fs::File;
use std::path::PathBuf;
use std::process::Command as PCommand;

static SMF_CONFIG_FILE_PROPERTY: &str = "config/file";
static SMF_FINISHED_PROPERTY: &str = "config/finished";

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    // File that holds the system config to apply
    #[clap(long, default_value = "/etc/sysconfig.json")]
    file: PathBuf,

    // SMF FMRI is an environment variable that we need. it is set by SMF by default
    #[clap(env)]
    smf_fmri: Option<String>,

    // Alternate root
    #[clap(short = 'R', long)]
    alt_root: Option<String>,
}

pub fn init_slog_logging(use_syslog: bool) -> Result<GlobalLoggerGuard> {
    if use_syslog {
        let drain = slog_syslog::unix_3164(Facility::LOG_DAEMON)?.fuse();
        let logger = Logger::root(drain, slog::slog_o!());

        let scope_guard = set_global_logger(logger);
        let _log_guard = slog_stdlog::init()?;

        Ok(scope_guard)
    } else {
        let decorator = TermDecorator::new().stdout().build();
        let drain = CompactFormat::new(decorator).build().fuse();
        let drain = Async::new(drain).build().fuse();
        let logger = Logger::root(drain, slog::slog_o!());

        let scope_guard = set_global_logger(logger);
        let _log_guard = slog_stdlog::init()?;

        Ok(scope_guard)
    }
}

fn main() -> Result<()> {
    let logger_guard: GlobalLoggerGuard;

    let cli: Cli = Cli::parse();

    if let Some(smf_fmri) = cli.smf_fmri.clone() {
        logger_guard = init_slog_logging(true)?;

        // Check if we have run before and exit if we did
        let cfg_finished_prop = libsysconfig::svcprop(SMF_FINISHED_PROPERTY, &smf_fmri)?;
        if let Some(finished) = cfg_finished_prop {
            if finished == String::from("true") {
                debug!(target: "sysconfig", "We have run before in this image exiting");
                return Ok(());
            }
        }
    } else {
        logger_guard = init_slog_logging(false)?;
    }

    let cfg_file_prop = if let Some(smf_fmri) = cli.smf_fmri.clone() {
        libsysconfig::svcprop(SMF_CONFIG_FILE_PROPERTY, &smf_fmri)?
    } else {
        None
    };
    debug!(target: "sysconfig", "Got config file {:?} from smf", cfg_file_prop);

    let cfg_file = if let Some(p) = cfg_file_prop {
        PathBuf::from(p)
    } else {
        cli.file
    };
    let log_string_cfg_file = cfg_file.clone().into_os_string().into_string();
    if log_string_cfg_file.is_ok() {
        debug!(target: "sysconfig", "config file is {}", log_string_cfg_file.unwrap());
    } else {
        debug!(target: "sysconfig", "config file is given but could not decode path to something that can be logged");
    }

    let mut parser = libcfgparser::SysConfigParser::default();
    for (key, v) in libsysconfig::get_supported_keywords() {
        trace!(target: "sysconfig", "Adding Keyword {} to parser", &key);
        parser.add_keyword(key, v);
    }

    debug!(target: "sysconfig", "Parsing config file");
    let instructions = if let Some(ext) = cfg_file.extension() {
        if ext == "json" {
            let f = File::open(cfg_file)?;
            debug!(target: "sysconfig", "Parsing JSON config");
            let set: InstructionsSet = serde_json::from_reader(f)?;
            set
        } else if ext == "yml" || ext == "yaml" {
            let f = File::open(cfg_file)?;
            debug!(target: "sysconfig", "Parsing YAML config");
            let set: InstructionsSet = serde_yaml::from_reader(f)?;
            set
        } else if ext == "ron" {
            let file_content = fs::read_to_string(cfg_file)?;
            debug!(target: "sysconfig", "Parsing RON config");
            let set: InstructionsSet = ron::from_str(&file_content)?;
            set
        } else {
            debug!(target: "sysconfig", "Parsing Custom sysconfig format");
            let keywords = parser.parse_config_file(cfg_file)?;
            libsysconfig::parse_keywords(keywords)?
        }
    } else {
        debug!(target: "sysconfig", "Parsing Custom sysconfig format");
        let keywords = parser.parse_config_file(cfg_file)?;
        libsysconfig::parse_keywords(keywords)?
    };

    // If we are nor running under SMF require an alternate root or mock
    let img = if cli.smf_fmri == None {
        if let Some(alt_root) = cli.alt_root {
            info!(target: "sysconfig", "Initializing to configure image mounted at {}", &alt_root);
            libsysconfig::Image::new(&alt_root)
        } else {
            // If we are not running under illumos SMF use a mocking driver
            info!(target: "sysconfig", "Initializing mock configuration for testing");
            libsysconfig::Image::new_with_driver("/", libsysconfig::Driver::Mock)
        }
    } else {
        info!(target: "sysconfig", "Initializing to configure live image");
        libsysconfig::Image::new("/")
    };

    // Apply configuration
    for instruction in instructions {
        let result = img.apply_instruction(instruction)?;
        trace!(target: "sysconfig", "Command result={:?}", result);
    }

    // If we run under SMF setup run blocker so we don't run a second time
    if let Some(smf_fmri) = cli.smf_fmri.clone() {
        info!(target: "sysconfig", "Finishing SMF run. Putting run guard into place");
        // rm -f /etc/.UNCONFIGURED
        debug!(target: "sysconfig", "Removing /etc/.UNCONFIGURED if it exists");
        let mut rm_cmd = PCommand::new("/usr/bin/rm");
        rm_cmd.arg("-f").arg("/etc/.UNCONFIGURED");

        // svccfg -s ${SMF_FMRI} "setprop config/finished=true"
        debug!(target: "sysconfig", "Setting SMF property config/finished=true");
        let finished_set_str = format!("{}=true", SMF_FINISHED_PROPERTY);
        libsysconfig::svccfg("/", vec!["-s", &smf_fmri, "setprop", &finished_set_str])?;

        // svccfg -s ${SMF_FMRI} "refresh"
        debug!(target: "sysconfig", "Refreshing SMF Service {}", &smf_fmri);
        libsysconfig::svccfg("/", vec!["-s", &smf_fmri, "refresh"])?;
    }

    drop(logger_guard);
    Ok(())
}
