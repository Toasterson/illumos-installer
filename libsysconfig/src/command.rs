use crate::{CommandOutput, InstructionError};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use log::debug;

static SVCCFG_BIN: &str = "/usr/sbin/svccfg";

pub fn run_command(root_path: &str, cmd_env: HashMap<&str,&str>, program: &str, args: Vec<&str>) -> Result<CommandOutput> {
    let mut cmd = Command::new(program);
    debug!(target: "libsysconfig", "Running Command {} with args={} and env={} in image rooted at {}",
    program, args.join(" "), cmd_env.iter().map(|(k, v)| format!("{}={}",k, v)).collect::<Vec<String>>().join(";"),
    root_path);
    
    let cmd = cmd.envs(cmd_env).args(args);
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

pub fn svccfg(root_path: &str, args: Vec<&str>) -> Result<CommandOutput> {
    let root_p = Path::new(root_path);

    let dtd_path = root_p
        .join("/usr/share/lib/xml/dtd/service_bundle.dtd.1")
        .to_string_lossy().to_string();

    let repo_path =  root_p.join("/etc/svc/repository.db").to_string_lossy().to_string();

    let svccfg_env = HashMap::from([
        ("SVCCFG_CHECKHASH", "1"),
        ("PKG_INSTALL_ROOT", root_path),
        ("SVCCFG_DTD", dtd_path.as_str()),
        ("SVCCFG_REPOSITORY", repo_path.as_str()),
        ("SVCCFG_CONFIGD_PATH", "/lib/svc/bin/svc.configd")
    ]);

    if root_path == "/" {
        debug!(target: "libsysconfig", "Executing svccfg with options {:?}", &args);
        run_command(root_path, HashMap::new(), SVCCFG_BIN, args)
    } else {
        debug!(target: "libsysconfig", "Executing svccfg with options {:?} in root {}", &args, root_path);
        run_command(root_path, svccfg_env, SVCCFG_BIN, args)
    }
}

pub fn svccfg_stdin(root_path: &str, stdin_content: String) -> Result<CommandOutput> {
    let root_p = Path::new(root_path);
    let repo_db = root_p.join("/etc/svc/repository.db");
    debug!(target: "libsysconfig", "Executing svccfg with stdin {}", &stdin_content);

    if root_path != "/" {
       debug!(target: "libsysconfig", "Excuting svccfg in alternate root {}", root_path);
    }

    let mut svccfg_child = Command::new(SVCCFG_BIN)
        .env("SVCCFG_CHECKHASH", "1")
        .env("PKG_INSTALL_ROOT", root_path)
        .env(
            "SVCCFG_DTD",
            root_p
                .join("/usr/share/lib/xml/dtd/service_bundle.dtd.1")
                .to_string_lossy()
                .into_owned(),
        )
        .env("SVCCFG_REPOSITORY", repo_db.to_string_lossy().into_owned())
        .env("SVCCFG_CONFIGD_PATH", "/lib/svc/bin/svc.configd")
        .stdin(Stdio::piped())
        .spawn()?;

    let mut stdin = svccfg_child.stdin.take().unwrap();
    std::thread::spawn(move || {
        stdin.write_all(stdin_content.as_bytes()).unwrap();
    });

    let output = svccfg_child.wait_with_output()?;

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
