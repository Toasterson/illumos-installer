use std::process::Command;
use anyhow::{anyhow, Result};

static SVCPROP_BIN: &str = "/usr/bin/svcprop";

pub fn svcprop(property: &str, smf_fmri: &str) -> Result<Option<String>> {
    let mut svcprop_cmd = Command::new(SVCPROP_BIN);
    svcprop_cmd.args(["-p", property, smf_fmri]);

    let output = svcprop_cmd.output()?;
    if output.status.success() {
        let str = String::from_utf8(output.stdout)?;
        if str.is_empty() {
            Ok(None)
        } else {
            Ok(Some(str.trim_end().to_string()))
        }
    } else {
        Err(anyhow!("svcprop command execution failed: {}", String::from_utf8(output.stderr)?))
    }
}