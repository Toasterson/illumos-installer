mod command;
mod illumos_driver;
mod keywords;
mod mock_driver;
mod devprop;

extern crate tera;

use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use libcfgparser::Keyword;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
pub use keywords::get_supported_keywords;

pub type InstructionsSet = Vec<Instruction>;

#[derive(Debug, Serialize, Deserialize)]
pub enum RootPasswordType {
    Clear(String),
    Hash(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum NetworkConfig {
    DHCP,
    DHCPStateful,
    DHCPStateless,
    Static(String),
}

//TODO Aggregate Setup
//TODO VLAN Setup
//TODO VNIC Setup
//TODO IPMP Setup
//TODO Etherstub Setup (mostly because VXLAN)
#[derive(Debug, Serialize, Deserialize)]
pub enum Instruction {
    CreateDataset {
        name: String,
        properties: Option<HashMap<String, String>>,
    },
    SetLocale {
        name: String,
        unicode: bool,
    },
    SetupDNS {
        domain: Option<String>,
        search: Option<String>,
        nameservers: Vec<String>,
    },
    AddRoute {
        name: String,
        route_match: String,
        gateway: String,
    },
    SetRootPassword(RootPasswordType),
    SetHostname(String),
    SetKeymap(String),
    SetTimezone(String),
    SetupTerminal {
        name: Option<String>,
        label: Option<String>,
        modules: Option<String>,
        prompt: Option<String>,
        terminal_type: String,
    },
    SetTimeServer(String),
    ConfigureNetworkAdapter {
        device: String,
        name: Option<String>,
        ipv4: Option<NetworkConfig>,
        ipv6: Option<NetworkConfig>,
        primary: bool,
    },
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct CommandOutput {
    command: String,
    root_path: String,
    output: String,
}

#[derive(Error, Debug)]
enum InstructionError {
    #[error("keyword {0} is not known")]
    UnknownInstruction(String),
    #[error("option {0} is not known for instruction {1}")]
    UnknownOptionInInstruction(String, String),
    #[error("applying instruction failed: command: {command} returned {output}")]
    CommandFailed { command: String, output: String },
    #[error("The root password has not been encrypted and hashed, aborting")]
    UnencryptedPassword,
}

pub fn parse_keywords(keywords: Vec<Keyword>) -> Result<InstructionsSet> {
    let mut set = InstructionsSet::new();
    for c in keywords {
        match c.name.as_str() {
            "keyboard" => {
                set.push(Instruction::SetKeymap(c.arguments[0].clone()));
            }
            "timezone" => {
                set.push(Instruction::SetTimezone(c.arguments[0].clone()));
            }
            "terminal" => {
                let (name_option, label_option, module_option, prompt_option, terminal_type) =
                    if let Some(opts) = c.options {
                        let mut name_option: Option<String> = None;
                        let mut label_option: Option<String> = None;
                        let mut module_option: Option<String> = None;
                        let mut prompt_option: Option<String> = None;
                        let mut terminal_type = String::new();

                        for (key, value) in opts {
                            match key.as_str() {
                                "name" => {
                                    name_option = Some(value);
                                }
                                "label" => {
                                    label_option = Some(value);
                                }
                                "module" => {
                                    module_option = Some(value);
                                }
                                "prompt" => {
                                    prompt_option = Some(value);
                                }
                                "type" => {
                                    terminal_type = value;
                                }
                                _ => {
                                    return Err(anyhow!(
                                        InstructionError::UnknownOptionInInstruction(c.name, key)
                                    ))
                                }
                            }
                        }

                        if terminal_type.is_empty() {
                            terminal_type = c.arguments[0].clone()
                        }

                        (
                            name_option,
                            label_option,
                            module_option,
                            prompt_option,
                            terminal_type,
                        )
                    } else {
                        (None, None, None, None, c.arguments[0].clone())
                    };
                set.push(Instruction::SetupTerminal {
                    name: name_option,
                    label: label_option,
                    modules: module_option,
                    prompt: prompt_option,
                    terminal_type,
                });
            }
            "timeserver" => {
                set.push(Instruction::SetTimeServer(c.arguments[0].clone()));
            }
            "network_interface" => {
                let parsed_options = if let Some(opts) = c.options {
                    let name_option = if opts.contains_key("name") {
                        Some(opts["name"].clone())
                    } else {
                        None
                    };
                    let (ipv4_option, ipv6_option) = if opts.contains_key("static") {
                        let static_addr = opts["static"].clone();
                        if static_addr.contains(":") {
                            (None, Some(NetworkConfig::Static(static_addr)))
                        } else if opts.contains_key("static6") {
                            let static6_addr = opts["static6"].clone();
                            (
                                Some(NetworkConfig::Static(static_addr)),
                                Some(NetworkConfig::Static(static6_addr)),
                            )
                        } else {
                            (Some(NetworkConfig::Static(static_addr)), None)
                        }
                    } else if opts.contains_key("static6") {
                        let static6_addr = opts["static6"].clone();
                        (None, Some(NetworkConfig::Static(static6_addr)))
                    } else {
                        (Some(NetworkConfig::DHCP), Some(NetworkConfig::DHCPStateful))
                    };

                    let primary_option = if opts.contains_key("primary") {
                        true
                    } else {
                        false
                    };

                    (name_option, ipv4_option, ipv6_option, primary_option)
                } else {
                    (None, None, None, false)
                };
                set.push(Instruction::ConfigureNetworkAdapter {
                    device: c.arguments[0].clone(),
                    name: parsed_options.0,
                    ipv4: parsed_options.1,
                    ipv6: parsed_options.2,
                    primary: parsed_options.3,
                })
            }
            "system_locale" => {
                let locale_name = c.arguments[0].clone();
                let unicode = if locale_name.to_uppercase().contains(".UTF-8") {
                    true
                } else if locale_name.contains(".") {
                    false
                } else {
                    true
                };
                set.push(Instruction::SetLocale {
                    name: locale_name,
                    unicode,
                });
            }
            "dataset" => {
                set.push(Instruction::CreateDataset {
                    name: c.arguments[0].clone(),
                    properties: c.options.clone(),
                });
            }
            "setup_dns" => {
                let (option_domain, option_search) = if let Some(opts) = c.options {
                    (
                        if opts.contains_key("domain") {
                            Some(opts["domain"].clone())
                        } else {
                            None
                        },
                        if opts.contains_key("search") {
                            Some(opts["search"].clone())
                        } else {
                            None
                        },
                    )
                } else {
                    (None, None)
                };
                set.push(Instruction::SetupDNS {
                    domain: option_domain,
                    search: option_search,
                    nameservers: c.arguments.clone(),
                });
            }
            "route" => {
                if c.arguments.len() > 2 {
                    set.push(Instruction::AddRoute {
                        name: c.arguments[0].clone(),
                        route_match: c.arguments[1].clone(),
                        gateway: c.arguments[2].clone(),
                    });
                } else {
                    set.push(Instruction::AddRoute {
                        name: c.arguments[0].clone(),
                        route_match: c.arguments[0].clone(),
                        gateway: c.arguments[1].clone(),
                    })
                }
            }
            "root_password" => {
                lazy_static! {
                    static ref RE: Regex = Regex::new(r"^\$\d\$").unwrap();
                }
                set.push(Instruction::SetRootPassword(
                    if RE.is_match(&c.arguments[0]) {
                        RootPasswordType::Hash(c.arguments[0].clone())
                    } else {
                        RootPasswordType::Hash(libshadow::gen_password_hash(
                            &c.arguments[0].clone(),
                        )?)
                    },
                ))
            }
            _ => {
                return Err(anyhow!(InstructionError::UnknownInstruction(
                    c.name.clone()
                )))
            }
        }
    }

    Ok(set)
}

pub enum Driver {
    Mock,
    Illumos,
}

pub struct Image<'a> {
    root_path: &'a str,
    driver: Driver,
}

impl<'a> Image<'a> {
    pub fn new(root_path: &'a str) -> Self {
        Image {
            root_path,
            driver: Driver::Illumos,
        }
    }

    pub fn new_with_driver(root_path: &'a str, driver: Driver) -> Self {
        Image { root_path, driver }
    }

    pub fn apply_instruction(&self, instruction: Instruction) -> Result<CommandOutput> {
        match self.driver {
            Driver::Mock => mock_driver::apply_instruction(self.root_path, instruction),
            Driver::Illumos => illumos_driver::apply_instruction(self.root_path, instruction),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
