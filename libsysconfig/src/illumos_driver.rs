use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;
use anyhow::{anyhow, Error, Result};
use lazy_static::lazy_static;
use crate::{CommandOutput, Instruction, RootPasswordType};
use crate::command::{run_command, svccfg, svccfg_stdin};
use crate::InstructionError;
use regex::Regex;
use thiserror::Error;
use libshadow::{parse_shadow_file, SHADOW_FILE};
use std::fs;
use serde_json::Value::String;

static ZFS_COMMAND: &str = "/usr/sbin/zfs";
static CP_COMMAND: &str = "/usr/bin/cp";
static SVCCFG_BIN: &str = "/usr/sbin/svccfg";
static DEFAULT_INIT_FILE: &str = "/etc/default/init";
static RESOLV_CONF_FILE: &str = "/etc/resolv.conf";
static NSSWITCH_CONF_FILE: &str = "/etc/nsswitch.conf";
static NSSWITCH_DNS_FILE: &str = "/etc/nsswitch.dns";

pub fn apply_instruction(root_path: &str, instruction: Instruction) -> Result<CommandOutput> {
    match instruction {
        Instruction::CreateDataset { name, properties } => {
            create_dataset(root_path, &name, properties)
        }
        Instruction::SetLocale { name, unicode } => {
            set_locale(root_path, &name, unicode)
        }
        Instruction::SetupDNS { domain, search, nameservers } => {
            setup_dns(root_path, nameservers, domain, search)
        }
        Instruction::AddRoute { name, route_match, gateway } => {

        }
        Instruction::SetRootPassword(tp) => {
            match tp {
                RootPasswordType::Clear(_) => {
                    Err(anyhow!(InstructionError::UnencryptedPassword))
                }
                RootPasswordType::Hash(hash) => {
                    set_root_password_hash(root_path, &hash)
                }
            }
        }
        Instruction::SetHostname(hostname) => {
            set_hostname(root_path, &hostname)
        }
        Instruction::SetKeymap(keymap) => {
            setup_keyboard(root_path, &keymap)
        }
        Instruction::SetTimezone(tz) => {
            setup_timezone(root_path, &tz)
        }
        Instruction::SetupTerminal{ name, label, modules, prompt, terminal_type} => {
            setup_terminal(root_path, name, label, modules, prompt, &terminal_type)
        }
        Instruction::SetTimeServer(time_server) => {

        }
        Instruction::ConfigureNetworkAdapter { device, name, ipv4, ipv6, primary } => {

        }
    }
}

fn set_hostname(root_path: &str, hostname: &str) -> Result<CommandOutput> {
    let r_path = Path::new(root_path);

    // /etc/nodename
    // /etc/inet/hosts

    Ok(CommandOutput {
        command: "internal".to_string(),
        root_path: root_path.clone().to_string(),
        output: "".to_string()
    })
}

fn set_root_password_hash(root_path: &str, hash: &str) -> Result<CommandOutput> {
    let p = Path::new(root_path);
    let shadow_path = p.join(SHADOW_FILE);
    let contents = fs::read_to_string(&shadow_path)?;

    let mut shadow = parse_shadow_file(&contents)?;
    if let Some(mut root_user) = shadow.get_entry("root") {
        root_user.set_password_hash(&hash);
        shadow.insert_or_update(root_user);

        let new_file = shadow.serialize();
        fs::write(&shadow_path, &new_file)?;
    }

    Ok(CommandOutput {
        command: "libshadow".to_string(),
        root_path: root_path.clone().to_string(),
        output: "".to_string()
    })
}

fn create_dataset(root_path:&str, name: &str, properties: Option<HashMap<String, String>>) -> Result<CommandOutput> {
    let mut zfs_cmd = Command::new(ZFS_COMMAND).arg("create");
    if let Some(props) = properties {
        for (key, value) in props {
            zfs_cmd.args(["-o", &format!("{}={}", key, value)]);
        }
    }
    zfs_cmd.arg(name);
    run_command(root_path, &mut zfs_cmd)
}

fn set_locale(root_path: &str, locale: &str, unicode: bool) -> Result<CommandOutput> {
    let locale = if unicode && !locale.contains("UTF-8") {
        let loc = locale.to_owned();
        loc + ".UTF-8";
        loc.as_str()
    } else {
        locale
    };

    let p = Path::new(root_path);

    let mut src = File::open(p.join(DEFAULT_INIT_FILE))?;
    let mut content = String::new();
    src.read_to_string(&mut content)?;
    drop(src);

    let lang_regex = Regex::new(r"^LANG=")?;
    let lang_str = format!("LANG={}", locale);
    let new_content = if lang_regex.is_match(&content) {
        lang_regex.replace_all(&content, lang_str).into()
    } else {
        content + "\n" + &lang_str
    };

    let mut dest = File::create(p.join(DEFAULT_INIT_FILE))?;
    dest.write(new_content.as_bytes())?;

    Ok(CommandOutput{
        command: "internal".to_string(),
        root_path: root_path.clone().to_string(),
        output: "".to_string()
    })
}

fn setup_dns(root_path: &str, nameservers: Vec<String>, domain: Option<String>, search: Option<String>) -> Result<CommandOutput> {
    let p = Path::new(root_path);
    let mut resolv_conf = String::new();
    for (iter, ns) in nameservers.iter().enumerate() {
        if iter > 0 {
            resolv_conf += format!("\n").as_str();
        }
        resolv_conf += format!("nameserver {}", ns).as_str();
    }

    if let Some(dom) = domain {
        resolv_conf += format!("\ndomain {}", dom).as_str();
    }

    if let Some(se) = search {
        resolv_conf += format!("\nsearch {}", se).as_str();
    }

    let mut dest = File::create(p.join(RESOLV_CONF_FILE))?;
    dest.write(resolv_conf.as_bytes())?;

    let mut nsswitch_dns_cp = Command::new(CP_COMMAND)
        .arg(p.join(NSSWITCH_DNS_FILE))
        .arg(p.join(NSSWITCH_CONF_FILE));

    run_command(root_path, nsswitch_dns_cp)
}

fn setup_keyboard(root_path: &str, keymap: &str) -> Result<CommandOutput> {
    let keyboard_command = vec![
            "-s".into(),
            "svc:/system/keymap:default".into(),
            "setprop".into(),
            format!("keymap/layout={}", keymap)
        ];

    svccfg(root_path, keyboard_command)
}

fn setup_timezone(root_path: &str, timezone: &str) -> Result<CommandOutput> {
    let p = Path::new(root_path);

    let mut src = File::open(p.join(DEFAULT_INIT_FILE))?;
    let mut content = String::new();
    src.read_to_string(&mut content)?;
    drop(src);

    let tz_regex = Regex::new(r"^TZ=")?;
    let tz_str = format!("TZ={}", timezone);
    let new_content = if tz_regex.is_match(&content) {
        tz_regex.replace_all(&content, tz_str).into()
    } else {
        content + "\n" + &tz_str
    };

    let mut dest = File::create(p.join(DEFAULT_INIT_FILE))?;
    dest.write(new_content.as_bytes())?;

    Ok(CommandOutput{
        command: "internal".to_string(),
        root_path: root_path.clone().to_string(),
        output: "".to_string()
    })
}

fn setup_terminal(root_path: &str, name: Option<String>, label: Option<String>, modules: Option<String>, prompt: Option<String>, terminal_type: &str) -> Result<CommandOutput> {

    if name == None && label == None && modules == None && prompt == None {
        let terminal_command = vec![
            "-s".into(),
            "svc:/system/console-login:default".into(),
            "setprop".into(),
            format!("ttymon/terminal_type={}", terminal_type)
        ];

        svccfg(root_path, terminal_command)
    } else {
        let mut stdin = String::new();
        if let Some(term_name) = name {
            stdin += "select svc:/system/console-login\n";
            stdin += &format!("add {}", term_name);
            stdin += &format!("select svc:/system/console-login:{}", term_name);
        } else {
            stdin += "select svc:/system/console-login:default";
        }
        stdin += "addpg ttymon application";
        if let Some(term_name) = name {
            stdin += &format!("setprop ttymon/device = astring: /dev/term/{}", term_name);
        }
        stdin += &format!("setprop ttymon/terminal_type = astring: {}", terminal_type);
        if let Some(term_label) = label {
            stdin += &format!("setprop ttymon/label = astring: {}", term_label);
        } else {
            stdin += &format!("setprop ttymon/label = astring: console");
        }

        if let Some(term_module) = modules {
            stdin += &format!("setprop ttymon/modules = astring: {}", term_module);
        } else {
            stdin += &format!("setprop ttymon/modules = astring: ldterm,ttcompat");
        }

        stdin += "setprop ttymon/nohangup = boolean: true";

        if let Some(term_prompt) = prompt {
            stdin += &format!("setprop ttymon/prompt = astring: \"{}\"", term_prompt);
        } else {
            stdin += &format!("setprop ttymon/prompt = astring: \"`uname -n` console login:\"");
        }

        stdin += "addpg general framework";

        svccfg_stdin(root_path, stdin)
    }

}