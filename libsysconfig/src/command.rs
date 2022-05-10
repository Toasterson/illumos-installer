use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use crate::{CommandOutput, InstructionError};
use anyhow::{anyhow, Result};

static SVCCFG_BIN: &str = "/usr/sbin/svccfg";

pub fn run_command(root_path: &str, cmd: &mut Command) -> Result<CommandOutput> {
    let output = cmd.output()?;
    if output.status.success() {
        Ok(CommandOutput {
            command: cmd.get_program().to_string_lossy().into_owned(),
            root_path: root_path.to_string(),
            output: String::from_utf8(output.stdout)?,
        })
    } else {
        Err(anyhow!(InstructionError::CommandFailed {
            command: cmd.get_program().to_string_lossy().into_owned(),
            output: String::from_utf8(output.stderr)?,
        }))
    }
}

pub fn svccfg(root_path: &str, args: Vec<String>) -> Result<CommandOutput> {
    let root_p = Path::new(root_path);
    let repo_db = root_p.join("/etc/svc/repository.db");

    let mut svccfg_cmd = Command::new(SVCCFG_BIN)
        .env("SVCCFG_CHECKHASH", "1")
        .env("PKG_INSTALL_ROOT", root_path)
        .env("SVCCFG_DTD", root_p.join("/usr/share/lib/xml/dtd/service_bundle.dtd.1").to_string_lossy().into_owned())
        .env("SVCCFG_REPOSITORY", repo_db.to_string_lossy().into_owned())
        .env("SVCCFG_CONFIGD_PATH", "/lib/svc/bin/svc.configd")
        .args(args);

    run_command(root_path, svccfg_cmd)
}

pub fn svccfg_stdin(root_path: &str, stdin_content: String) -> Result<CommandOutput> {
    let root_p = Path::new(root_path);
    let repo_db = root_p.join("/etc/svc/repository.db");

    let mut svccfg_child = Command::new(SVCCFG_BIN)
        .env("SVCCFG_CHECKHASH", "1")
        .env("PKG_INSTALL_ROOT", root_path)
        .env("SVCCFG_DTD", root_p.join("/usr/share/lib/xml/dtd/service_bundle.dtd.1").to_string_lossy().into_owned())
        .env("SVCCFG_REPOSITORY", repo_db.to_string_lossy().into_owned())
        .env("SVCCFG_CONFIGD_PATH", "/lib/svc/bin/svc.configd")
        .stdin(Stdio::piped())
        .spawn()?;

    let mut stdin = svccfg_child.stdin.take()?;
    std::thread::spawn(move || {
        stdin.write_all(stdin_content.as_bytes())?;
    });

    let output = child.wait_with_output()?;

    if output.status.success() {
        Ok(CommandOutput {
            command: SVCCFG_BIN.clone().to_string(),
            root_path: root_path.to_string(),
            output: String::from_utf8(output.stdout)?,
        })
    } else {
        Err(anyhow!(InstructionError::CommandFailed {
            command: SVCCFG_BIN.clone().to_string(),
            output: String::from_utf8(output.stderr)?,
        }))
    }
}