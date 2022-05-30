use crate::command::{run_command, svccfg, svccfg_stdin};
use crate::InstructionError;
use crate::{CommandOutput, Instruction, NetworkConfig, RootPasswordType};
use anyhow::{anyhow, Result};
use libshadow::{parse_shadow_file, SHADOW_FILE};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use tera::{Context, Tera};

static ZFS_COMMAND: &str = "/usr/sbin/zfs";
static CP_COMMAND: &str = "/usr/bin/cp";
static ROUTE_BIN: &str = "/usr/sbin/route";
static IPADM_BIN: &str = "/usr/sbin/ipadm";
static DEFAULT_INIT_FILE: &str = "/etc/default/init";
static RESOLV_CONF_FILE: &str = "/etc/resolv.conf";
static NSSWITCH_CONF_FILE: &str = "/etc/nsswitch.conf";
static NSSWITCH_DNS_FILE: &str = "/etc/nsswitch.dns";
static NODENAME_FILE: &str = "/etc/nodename";
static INET_HOSTS_FILE: &str = "/etc/inet/hosts";
static INET_HOSTS_TEMPLATE: &str = r#"# CDDL HEADER START
#
# The contents of this file are subject to the terms of the
# Common Development and Distribution License (the "License").
# You may not use this file except in compliance with the License.
#
# You can obtain a copy of the license at usr/src/OPENSOLARIS.LICENSE
# or http://www.opensolaris.org/os/licensing.
# See the License for the specific language governing permissions
# and limitations under the License.
#
# When distributing Covered Code, include this CDDL HEADER in each
# file and include the License file at usr/src/OPENSOLARIS.LICENSE.
# If applicable, add the following below this CDDL HEADER, with the
# fields enclosed by brackets "[]" replaced with your own identifying
# information: Portions Copyright [yyyy] [name of copyright owner]
#
# CDDL HEADER END
#
# Copyright 2009 Sun Microsystems, Inc.  All rights reserved.
# Use is subject to license terms.
#
# Internet host table
#
::1 {{hostname}} {{hostname}}.local localhost loghost
127.0.0.1 {{hostname}} {{hostname}}.local localhost loghost
"#;

pub fn apply_instruction(root_path: &str, instruction: Instruction) -> Result<CommandOutput> {
    match instruction {
        Instruction::CreateDataset { name, properties } => {
            create_dataset(root_path, &name, properties)
        }
        Instruction::SetLocale { name, unicode } => set_locale(root_path, &name, unicode),
        Instruction::SetupDNS {
            domain,
            search,
            nameservers,
        } => setup_dns(root_path, nameservers, domain, search),
        Instruction::AddRoute {
            route_match,
            gateway,
            ..
        } => add_route(root_path, route_match, gateway),
        Instruction::SetRootPassword(tp) => match tp {
            RootPasswordType::Clear(_) => Err(anyhow!(InstructionError::UnencryptedPassword)),
            RootPasswordType::Hash(hash) => set_root_password_hash(root_path, &hash),
        },
        Instruction::SetHostname(hostname) => set_hostname(root_path, &hostname),
        Instruction::SetKeymap(keymap) => setup_keyboard(root_path, &keymap),
        Instruction::SetTimezone(tz) => setup_timezone(root_path, &tz),
        Instruction::SetupTerminal {
            name,
            label,
            modules,
            prompt,
            terminal_type,
        } => setup_terminal(root_path, name, label, modules, prompt, &terminal_type),
        Instruction::SetTimeServer(_) => {
            unimplemented!()
        }
        Instruction::ConfigureNetworkAdapter {
            device,
            name,
            ipv4,
            ipv6,
            primary,
        } => setup_interface(root_path, device, name, ipv4, ipv6, primary),
    }
}

fn setup_interface(
    root_path: &str,
    device: String,
    name: Option<String>,
    ipv4: Option<NetworkConfig>,
    ipv6: Option<NetworkConfig>,
    primary: bool,
) -> Result<CommandOutput> {
    #[allow(unused_assignments)]
    let mut v4_static = String::new();
    #[allow(unused_assignments)]
    let mut v6_static = String::new();
    let mut addr_conf = if let Some(ipv4_conf) = ipv4 {
        match ipv4_conf {
            NetworkConfig::Static(v4_static_1) => {
                v4_static = v4_static_1;
                if primary {
                    vec!["-T", "static", "-1", "-a", &v4_static]
                } else {
                    vec!["-T", "static", "-a", &v4_static]
                }
            }
            _ => {
                vec!["-T", "dhcp"]
            }
        }
    } else if let Some(ipv6_conf) = ipv6.clone() {
        let dev_name_addrconf = device.clone() + "/v6_local";
        match ipv6_conf {
            NetworkConfig::DHCP => {
                vec!["-T", "dhcp"]
            }
            NetworkConfig::DHCPStateful => {
                let ipadm_addrconf_args = vec![
                    "-R",
                    root_path,
                    "-T", "addrconf", "-p", "stateful=yes",
                    &dev_name_addrconf];
                run_command(root_path, HashMap::new(), IPADM_BIN, ipadm_addrconf_args)?;
                vec!["-T", "dhcp"]
            }
            NetworkConfig::DHCPStateless => {
                let ipadm_addrconf_args = vec![
                    "-R",
                    root_path,
                    "-T", "addrconf", "-p", "stateless=yes",
                    &dev_name_addrconf];
                run_command(root_path, HashMap::new(), IPADM_BIN, ipadm_addrconf_args)?;
                vec!["-T", "dhcp"]
            }
            NetworkConfig::Static(v6_addr_1) => {
                v6_static = v6_addr_1;
                let ipadm_addrconf_args = vec![
                    "-R",
                    root_path,
                    "-T", "addrconf", "-p", "stateless=yes",
                    &dev_name_addrconf];
                run_command(root_path, HashMap::new(), IPADM_BIN, ipadm_addrconf_args)?;
                vec!["-T", "static", "-a", &v6_static]
            }
        }
    } else {
        vec!["-T", "dhcp"]
    };

    let dev_name = if let Some(name) = name {
        device + &name
    } else {
        if !ipv6.is_none() {
            device + "/v6"
        } else {
            device + "/v4"
        }
    };

    let mut ipadm_args = vec![
        "-R",
        root_path];
    ipadm_args.append(&mut addr_conf);
    ipadm_args.push(&dev_name);

    run_command(root_path, HashMap::new(), IPADM_BIN, ipadm_args)
}

fn add_route(root_path: &str, route_match: String, gateway: String) -> Result<CommandOutput> {
    let route_args = vec![
        "-R",
        root_path,
        "-p",
        &route_match,
        &gateway];

    run_command(root_path, HashMap::new(), ROUTE_BIN, route_args)
}

fn set_hostname(root_path: &str, hostname: &str) -> Result<CommandOutput> {
    let r_path = Path::new(root_path);

    // /etc/nodename
    let nodename = hostname.to_string() + "\n";
    let mut nodename_dest = File::create(r_path.join(NODENAME_FILE))?;
    nodename_dest.write(nodename.as_bytes())?;

    // /etc/inet/hosts
    let mut context = Context::new();
    context.insert("hostname", hostname);
    let inet_hosts_content = Tera::one_off(INET_HOSTS_TEMPLATE, &context, true)?;
    let mut inet_hosts_dest = File::create(r_path.join(INET_HOSTS_FILE))?;
    inet_hosts_dest.write(inet_hosts_content.as_bytes())?;

    Ok(CommandOutput {
        command: "internal".to_string(),
        root_path: root_path.clone().to_string(),
        output: "".to_string(),
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
        output: "".to_string(),
    })
}

fn create_dataset(
    root_path: &str,
    name: &str,
    properties: Option<HashMap<String, String>>,
) -> Result<CommandOutput> {
    let mut zfs_args = vec!["create"];
    let mut prop_args: Vec<String> = vec![];
    if let Some(props) = properties {
        for (key, value) in props {
            let pair = format!("{}={}", key, value);
            prop_args.append(&mut vec!["-o".into(), pair]);
        }
    }
    let mut p = prop_args.iter_mut().map(|p| p.as_str()).collect::<Vec<&str>>();
    zfs_args.append(&mut p);
    zfs_args.push(name);
    run_command(root_path, HashMap::new(), ZFS_COMMAND, zfs_args)
}

fn set_locale(root_path: &str, locale: &str, unicode: bool) -> Result<CommandOutput> {
    let locale = if unicode && !locale.contains("UTF-8") {
        let mut loc = String::from(locale.clone());
        loc += ".UTF-8";
        loc
    } else {
        String::from(locale)
    };
    // TODO: Fix multiple lang lines in File when run multiple times
    let p = Path::new(root_path);

    let mut src = File::open(p.join(DEFAULT_INIT_FILE))?;
    let mut content = String::new();
    src.read_to_string(&mut content)?;
    drop(src);

    let lang_regex = Regex::new(r"^LANG=")?;
    let lang_str = format!("LANG={}\n", locale);
    let new_content = if lang_regex.is_match(&content) {
        lang_regex.replace_all(&content, lang_str).into()
    } else {
        content + "\n" + &lang_str
    };

    let mut dest = File::create(p.join(DEFAULT_INIT_FILE))?;
    dest.write(new_content.as_bytes())?;

    Ok(CommandOutput {
        command: "internal".to_string(),
        root_path: root_path.clone().to_string(),
        output: "".to_string(),
    })
}

fn setup_dns(
    root_path: &str,
    nameservers: Vec<String>,
    domain: Option<String>,
    search: Option<String>,
) -> Result<CommandOutput> {
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

    let nsswitch_dns_fullpath = p.join(NSSWITCH_DNS_FILE).to_string_lossy().to_string().clone();
    let nsswitch_conf_fullpath = p.join(NSSWITCH_CONF_FILE).to_string_lossy().to_string().clone();

    let nsswitch_dns_cp = vec![
        nsswitch_dns_fullpath.as_str(),
        nsswitch_conf_fullpath.as_str()];

    run_command(root_path, HashMap::new(), CP_COMMAND,nsswitch_dns_cp)
}

fn setup_keyboard(root_path: &str, keymap: &str) -> Result<CommandOutput> {
    let keymap_layout_arg = format!("keymap/layout={}", keymap);
    let keyboard_command = vec![
        "-s",
        "svc:/system/keymap:default",
        "setprop",
        &keymap_layout_arg,
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

    Ok(CommandOutput {
        command: "internal".to_string(),
        root_path: root_path.clone().to_string(),
        output: "".to_string(),
    })
}

fn setup_terminal(
    root_path: &str,
    name: Option<String>,
    label: Option<String>,
    modules: Option<String>,
    prompt: Option<String>,
    terminal_type: &str,
) -> Result<CommandOutput> {
    if name == None && label == None && modules == None && prompt == None {
        let ttymon_arg = format!("ttymon/terminal_type={}", terminal_type);
        let terminal_command = vec![
            "-s",
            "svc:/system/console-login:default",
            "setprop",
            &ttymon_arg,
        ];

        svccfg(root_path, terminal_command)
    } else {
        let mut stdin = String::new();
        if let Some(term_name) = name.clone() {
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
