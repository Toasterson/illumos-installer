use crate::InstructionError;
use crate::{CommandOutput, Instruction, NetworkConfig, RootPasswordType};
use anyhow::{anyhow, Result};
use illumos::{run, run_capture_stdout, svccfg};
use libshadow::{parse_shadow_file, SHADOW_FILE};
use log::{debug, info, warn};
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

// TODO: Switch root_path to Optional<&str>
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
                info!(target: "libsysconfig", "Device {} is being setup with IPv4 Address {}", &device, &v4_static_1);
                v4_static = v4_static_1;
                if primary {
                    vec!["-T", "static", "-1", "-a", &v4_static]
                } else {
                    vec!["-T", "static", "-a", &v4_static]
                }
            }
            _ => {
                info!(target: "libsysconfig", "Device {} is being set to DHCP", &device);
                vec!["-T", "dhcp"]
            }
        }
    } else if let Some(ipv6_conf) = ipv6.clone() {
        let dev_name_addrconf = device.clone() + "/v6_local";
        match ipv6_conf {
            NetworkConfig::DHCP => {
                info!(target: "libsysconfig", "Device {} is being set to DHCPv6", &device);
                vec!["-T", "dhcp"]
            }
            NetworkConfig::DHCPStateful => {
                info!(target: "libsysconfig", "Device {} is being set to DHCPv6 Stateful", &device);
                let ipadm_addrconf_args = vec![
                    IPADM_BIN,
                    "-R",
                    root_path,
                    "-T",
                    "addrconf",
                    "-p",
                    "stateful=yes",
                    &dev_name_addrconf,
                ];
                run(&ipadm_addrconf_args, None)?;
                vec!["-T", "dhcp"]
            }
            NetworkConfig::DHCPStateless => {
                info!(target: "libsysconfig", "Device {} is being set to DHCPv6 Stateless", &device);
                let ipadm_addrconf_args = vec![
                    IPADM_BIN,
                    "-R",
                    root_path,
                    "-T",
                    "addrconf",
                    "-p",
                    "stateless=yes",
                    &dev_name_addrconf,
                ];
                run(&ipadm_addrconf_args, None)?;
                vec!["-T", "dhcp"]
            }
            NetworkConfig::Static(v6_addr_1) => {
                v6_static = v6_addr_1;
                info!(target: "libsysconfig", "Device {} is being set to IPv6 Static Address {}", &device, &v6_static);
                let ipadm_addrconf_args = vec![
                    IPADM_BIN,
                    "-R",
                    root_path,
                    "-T",
                    "addrconf",
                    "-p",
                    "stateless=yes",
                    &dev_name_addrconf,
                ];
                run(&ipadm_addrconf_args, None)?;
                info!(target: "libsysconfig", "Device {} will have addrconf configured", &device);
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

    let mut ipadm_args = vec![IPADM_BIN, "-R", root_path];
    ipadm_args.append(&mut addr_conf);
    ipadm_args.push(&dev_name);

    Ok(CommandOutput {
        command: ipadm_args.join(" "),
        root_path: root_path.to_string(),
        output: run_capture_stdout(&ipadm_args, None)?,
    })
}

fn add_route(root_path: &str, route_match: String, gateway: String) -> Result<CommandOutput> {
    let route_args = vec![ROUTE_BIN, "-R", root_path, "-p", &route_match, &gateway];
    info!(target: "libsysconfig", "Adding route {}->{} to system mounted at {}", &route_match, &gateway, root_path);
    Ok(CommandOutput {
        command: ipadm_args.join(" "),
        root_path: root_path.to_string(),
        output: run_capture_stdout(&route_args, None)?,
    })
}

fn set_hostname(root_path: &str, hostname: &str) -> Result<CommandOutput> {
    let r_path = Path::new(root_path);

    info!(target: "libsysconfig", "Setting hostname to {}", hostname);
    // /etc/nodename
    let nodename = hostname.to_string() + "\n";
    let mut nodename_dest = File::create(r_path.join(NODENAME_FILE))?;
    nodename_dest.write(nodename.as_bytes())?;
    debug!(target: "libsysconfig", "Updated {}/etc/nodename", root_path);

    // /etc/inet/hosts
    let mut context = Context::new();
    context.insert("hostname", hostname);
    let inet_hosts_content = Tera::one_off(INET_HOSTS_TEMPLATE, &context, true)?;
    let mut inet_hosts_dest = File::create(r_path.join(INET_HOSTS_FILE))?;
    inet_hosts_dest.write(inet_hosts_content.as_bytes())?;
    debug!(target: "libsysconfig", "Updated {}/etc/inet/hosts", root_path);

    Ok(CommandOutput {
        command: "internal".to_string(),
        root_path: root_path.clone().to_string(),
        output: "success".to_string(),
    })
}

fn set_root_password_hash(root_path: &str, hash: &str) -> Result<CommandOutput> {
    let p = Path::new(root_path);
    let shadow_path = p.join(SHADOW_FILE);
    let contents = fs::read_to_string(&shadow_path)?;
    info!(target: "libsysconfig", "Setting root password to hash given");
    let mut shadow = parse_shadow_file(&contents)?;
    if let Some(mut root_user) = shadow.get_entry("root") {
        root_user.set_password_hash(&hash);
        shadow.insert_or_update(root_user);

        let new_file = shadow.serialize();
        fs::write(&shadow_path, &new_file)?;
    } else {
        warn!(target: "libsysconfig", "No root user present in shadow file skipping setting the password")
    }

    Ok(CommandOutput {
        command: "libshadow".to_string(),
        root_path: root_path.clone().to_string(),
        output: "success".to_string(),
    })
}

fn create_dataset(
    root_path: &str,
    name: &str,
    properties: Option<HashMap<String, String>>,
) -> Result<CommandOutput> {
    let mut zfs_args = vec![ZFS_COMMAND, "create"];
    let mut prop_args: Vec<String> = vec![];
    if let Some(props) = properties {
        info!(target: "libsysconfig", "Creating Dataset {} with properties={}", name,
        &props.iter().map(|(k,v)| format!("{}={}",k,v)).collect::<Vec<String>>().join(";"));

        for (key, value) in props {
            let pair = format!("{}={}", key, value);
            prop_args.append(&mut vec!["-o".into(), pair]);
        }
    } else {
        info!(target: "libsysconfig", "Creating Dataset {}", name);
    }
    let mut p = prop_args
        .iter_mut()
        .map(|p| p.as_str())
        .collect::<Vec<&str>>();
    zfs_args.append(&mut p);
    zfs_args.push(name);
    Ok(CommandOutput {
        command: zfs_args.join(" ").to_string(),
        root_path: root_path.to_string(),
        output: run_capture_stdout(&zfs_args, None)?,
    })
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
    info!(target: "libsysconfig", "Setting LANG={}", locale.clone());
    let mut src = File::open(p.join(DEFAULT_INIT_FILE))?;
    let mut content = String::new();
    src.read_to_string(&mut content)?;
    drop(src);

    let mut new_content = String::new();
    for line in content.lines() {
        // Only do something if the line starts with LANG=
        if line.starts_with("LANG") {
            // Return Success if the locale is already set correctly
            if line == "LANG=".to_string() + &locale {
                debug!(target: "libsysconfig", "Locale already correct returning");
                return Ok(CommandOutput {
                    command: "internal".to_string(),
                    root_path: root_path.clone().to_string(),
                    output: "".to_string(),
                });
            } else {
                new_content += &format!("LANG={}", &locale);
            }
        } else {
            new_content += line;
        }
        new_content += "\n";
    }
    debug!(target: "libsysconfig", "New content of {}/{} is \n {}", root_path, DEFAULT_INIT_FILE, &new_content);

    let mut dest = File::create(p.join(DEFAULT_INIT_FILE))?;
    dest.write(new_content.as_bytes())?;

    Ok(CommandOutput {
        command: "internal".to_string(),
        root_path: root_path.clone().to_string(),
        output: "success".to_string(),
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
        info!(target: "libsysconfig", "Adding DNS server {}", &ns);
        resolv_conf += format!("nameserver {}", ns).as_str();
    }

    if let Some(dom) = domain {
        info!(target: "libsysconfig", "Setting DNS domain to {}", &dom);
        resolv_conf += format!("\ndomain {}", dom).as_str();
    }

    if let Some(se) = search {
        info!(target: "libsysconfig", "Setting DNS search to {}", &se);
        resolv_conf += format!("\nsearch {}", se).as_str();
    }

    let mut dest = File::create(p.join(RESOLV_CONF_FILE))?;
    dest.write(resolv_conf.as_bytes())?;

    let nsswitch_dns_fullpath = p
        .join(NSSWITCH_DNS_FILE)
        .to_string_lossy()
        .to_string()
        .clone();
    let nsswitch_conf_fullpath = p
        .join(NSSWITCH_CONF_FILE)
        .to_string_lossy()
        .to_string()
        .clone();

    let nsswitch_dns_cp = vec![
        nsswitch_dns_fullpath.as_str(),
        nsswitch_conf_fullpath.as_str(),
    ];

    run_command(root_path, HashMap::new(), CP_COMMAND, nsswitch_dns_cp)
}

fn setup_keyboard(root_path: &str, keymap: &str) -> Result<CommandOutput> {
    info!(target: "libsysconfig", "Setting Keyboard layout to {}", keymap);
    let keymap_layout_arg = format!("keymap/layout={}", keymap);
    let keyboard_command = vec![
        "select svc:/system/keymap:default",
        "setprop",
        &keymap_layout_arg,
    ];

    let alt_root = if root_path == "/" {
        None
    } else {
        Some(root_path)
    };

    svccfg(&keyboard_command, alt_root)?;

    Ok(CommandOutput {
        command: keyboard_command.join(" "),
        root_path: root_path.to_string(),
        output: String::new(),
    })
}

fn setup_timezone(root_path: &str, timezone: &str) -> Result<CommandOutput> {
    let p = Path::new(root_path);

    info!(target: "libsysconfig", "Setting timezone to {}", timezone);
    let mut src = File::open(p.join(DEFAULT_INIT_FILE))?;
    let mut content = String::new();
    src.read_to_string(&mut content)?;
    drop(src);

    let mut new_content = String::new();
    for line in content.lines() {
        // Only do something if the line starts with LANG=
        if line.starts_with("TZ") {
            // Return Success if the locale is already set correctly
            if line == "TZ=".to_string() + &timezone {
                debug!(target: "libsysconfig", "Timezone already correct returning");
                return Ok(CommandOutput {
                    command: "internal".to_string(),
                    root_path: root_path.clone().to_string(),
                    output: "".to_string(),
                });
            } else {
                new_content += &format!("TZ={}", &timezone);
            }
        } else {
            new_content += line;
        }
        new_content += "\n";
    }
    debug!(target: "libsysconfig", "New content of {}/{} is \n {}", root_path, DEFAULT_INIT_FILE, &new_content);

    let mut dest = File::create(p.join(DEFAULT_INIT_FILE))?;
    dest.write(new_content.as_bytes())?;

    Ok(CommandOutput {
        command: "internal".to_string(),
        root_path: root_path.clone().to_string(),
        output: String::new(),
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
    let alt_root = if root_path == "/" {
        None
    } else {
        Some(root_path)
    };

    if name == None && label == None && modules == None && prompt == None {
        info!(target: "libsysconfig", "Setting terminal type to {}", terminal_type);
        let ttymon_arg = format!("setprop ttymon/terminal_type = astring: {}", terminal_type);
        let terminal_args = vec!["select svc:/system/console-login", &ttymon_arg];
        svccfg(&terminal_args, alt_root)?;

        Ok(CommandOutput {
            command: terminal_args.join(";"),
            root_path: root_path.to_string(),
            output: String::new(),
        })
    } else {
        info!(target: "libsysconfig", "Setting terminal up with configuration name={:?} label={:?} modules={:?} prompt={:?} type={}",
        name, label, modules, prompt, terminal_type);

        let mut terminal_args: Vec<&str> = vec![];
        if let Some(term_name) = name.clone() {
            terminal_args.push("select svc:/system/console-login");
            terminal_args.push(&format!("add {}", term_name));
            terminal_args.push(&format!("select svc:/system/console-login:{}", term_name));
        } else {
            terminal_args.push("select svc:/system/console-login");
        }
        terminal_args.push("addpg ttymon application");
        if let Some(term_name) = name {
            terminal_args.push(&format!(
                "setprop ttymon/device = astring: /dev/term/{}",
                term_name
            ));
        }
        terminal_args.push(&format!(
            "setprop ttymon/terminal_type = astring: {}",
            terminal_type
        ));
        if let Some(term_label) = label {
            terminal_args.push(&format!("setprop ttymon/label = astring: {}", term_label));
        } else {
            terminal_args.push(&format!("setprop ttymon/label = astring: console"));
        }

        if let Some(term_module) = modules {
            terminal_args.push(&format!(
                "setprop ttymon/modules = astring: {}",
                term_module
            ));
        } else {
            terminal_args.push(&format!(
                "setprop ttymon/modules = astring: ldterm,ttcompat"
            ));
        }

        terminal_args.push("setprop ttymon/nohangup = boolean: true");

        if let Some(term_prompt) = prompt {
            terminal_args.push(&format!(
                "setprop ttymon/prompt = astring: \"{}\"",
                term_prompt
            ));
        } else {
            terminal_args.push(&format!(
                "setprop ttymon/prompt = astring: \"`uname -n` console login:\""
            ));
        }

        terminal_args.push("addpg general framework");

        svccfg(&terminal_args, alt_root)?;

        Ok(CommandOutput {
            command: terminal_args.join(";"),
            root_path: root_path.to_string(),
            output: String::new(),
        })
    }
}
