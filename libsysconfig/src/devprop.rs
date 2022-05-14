use std::process::Command;
use anyhow::{anyhow, Result};

#[allow(dead_code)]
static DEVPROP_BIN: &str = "/sbin/devprop";

#[allow(dead_code)]
pub fn get_key(key: &str) -> Result<String> {
    let mut cmd = Command::new(DEVPROP_BIN);
    let cmd_args = cmd.arg(key);

    let output = cmd_args.output()?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?)
    } else {
        Err(anyhow!(String::from_utf8(output.stderr)?))
    }
}