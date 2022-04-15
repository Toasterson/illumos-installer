pub type InstructionsSet = Vec<InstallInstruction>;

#[derive(Debug)]
pub enum InstallInstruction {
    CreatePool {
        vdevs: Vec<VDEVConfiguration>,
        pool_options: Vec<(String, String)>,
    },
    SetLocale {
        name: String,
        unicode: bool,
    },
    CreateBootEnvironment(String),
    InstallImage {
        url: String,
        image_options: Option<Vec<(String, String)>>,
    },
    CreateDataset {
        name: String,
        mount_options: Vec<(String, String)>,
    },
    AddDNSServer(String),
    SetDNSDomain(String),
    AddDNSSearch(String),
    AddRoute {
        name: String,
        route_match: String,
        gateway: String,
    },
    SetRootPassword {
        clear: Option<String>,
        encrypted: Option<String>,
    },
    SetHostname(String),
    SetKeymap(String),
    SetTimezone(String),
    ConfigureNetworkAdapter {
        net_type: String,
        name: String,
        device: String,
        ipv4: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum VDEVType {
    Empty,
    Mirror,
    RaidZ1,
    RaidZ2,
    RaidZ3
}

impl Default for VDEVType {
    fn default() -> Self {
        Self::Empty
    }
}

#[derive(Debug, Default, Clone)]
pub struct VDEVConfiguration {
    pub vdev_type: VDEVType,
    pub devices: Vec<String>
}

#[derive(Debug)]
pub enum Config {
    Instruction(String, Option<Vec<(String, String)>>, Vec<String>),
    Command(String),
    Option(String),
    Argument(String, String),
    Value(String),
    EOI,
}

use pest::Parser;
use anyhow::{anyhow, Result};
use thiserror::Error;
use crate::config::InstructionError::{BadConfigParsed, UnknownInstruction};

#[derive(Parser)]
#[grammar = "config.pest"]
struct ConfigParser;

pub fn parse_config(file: &str) -> Result<Vec<Config>> {
    let config = ConfigParser::parse(Rule::config, file)?.next().unwrap();

    use pest::iterators::Pair;

    fn parse_value(pair: Pair<Rule>) -> Config {
        match pair.as_rule() {
            Rule::command => {
                let mut cmd_str = String::new();
                let mut arguments: Option<Vec<(String, String)>> = None;
                let mut options: Vec<String> = vec![];
                for pair in pair.into_inner() {
                    match parse_value(pair) {
                        Config::Command(s) => cmd_str = s,
                        Config::Option(opt) => options.push(opt),
                        Config::Argument(name, val) => {
                            arguments = if let Some(mut args) = arguments.clone() {
                                args.push((name, val));
                                Some(args)
                            } else {
                                Some(vec![(name, val)])
                            };
                        }
                        _ => panic!(),
                    }
                }
                Config::Instruction(
                    cmd_str,
                    arguments,
                    options
                )
            }
            Rule::command_word => Config::Command(pair.as_str().into()),
            Rule::command_argument => {
                let mut arg_name = String::new();
                let mut arg_value = String::new();
                for p in pair.into_inner() {
                    match parse_value(p) {
                        Config::Command(cmd) => {
                            arg_name= cmd
                        }
                        Config::Value(val) => {
                            arg_value = val
                        }
                        _ => panic!(),
                    }
                }
                Config::Argument(arg_name, arg_value)
            },
            Rule::string => {
                Config::Value(pair.into_inner().next().unwrap().as_str().into())
            }
            Rule::command_option => {
                let inner_pair = pair.into_inner().next().unwrap();
                match inner_pair.as_rule() {
                    Rule::quoteless_string => Config::Option(inner_pair.as_str().into()),
                    Rule::string => Config::Option(inner_pair.into_inner().next().unwrap().as_str().into()),
                    _ => panic!(),
                }
            }
            Rule::config => {
                let inner_pair = pair.into_inner().next().unwrap();
                let config = parse_value(inner_pair);
                config
            }
            Rule::quoteless_string
            | Rule::inner
            | Rule::char
            | Rule::COMMENT
            | Rule::WHITESPACE => panic!(),
            Rule::EOI => Config::EOI,
        }
    }

    let mut instructions: Vec<Config> = vec![];

    for pair in config.into_inner() {
        let config = parse_value(pair);
        instructions.push(config);
    }

    Ok(instructions)
}

#[derive(Error, Debug)]
enum InstructionError {
    #[error("instruction {0} is not known")]
    UnknownInstruction(String),
    #[error("config parsed badly reached non instruction")]
    BadConfigParsed,
}

pub fn parse_config_to_instructions(instructions: Vec<Config>) -> Result<InstructionsSet> {
    let mut set = InstructionsSet::new();
    for c in instructions {
        match c {
            Config::Instruction(cmd, args, options) => {
                match cmd.as_str() {
                    "zpool-create" => {
                        let mut vdevs: Vec<VDEVConfiguration> = vec![];
                        let mut pool_options: Vec<(String, String)> = vec![];
                        if let Some(args) = args {
                            for (name, value) in args {
                                pool_options.push((name, value))
                            }
                        }

                        let mut vdev_config = VDEVConfiguration::default();
                        for opt in options {
                            match opt.as_str() {
                                "mirror" => {
                                    if vdev_config.vdev_type != VDEVType::Empty {
                                        vdevs.push(vdev_config.clone());
                                        vdev_config = VDEVConfiguration::default();
                                    }
                                    vdev_config.vdev_type = VDEVType::Mirror
                                }
                                "raidz" | "raidz1" => {
                                    if vdev_config.vdev_type != VDEVType::Empty {
                                        vdevs.push(vdev_config.clone());
                                        vdev_config = VDEVConfiguration::default();
                                    }
                                    vdev_config.vdev_type = VDEVType::RaidZ1
                                }
                                "raidz2" => {
                                    if vdev_config.vdev_type != VDEVType::Empty {
                                        vdevs.push(vdev_config.clone());
                                        vdev_config = VDEVConfiguration::default();
                                    }
                                    vdev_config.vdev_type = VDEVType::RaidZ2
                                }
                                "raidz3" => {
                                    if vdev_config.vdev_type != VDEVType::Empty {
                                        vdevs.push(vdev_config.clone());
                                        vdev_config = VDEVConfiguration::default();
                                    }
                                    vdev_config.vdev_type = VDEVType::RaidZ3
                                }
                                _ => {
                                    vdev_config.devices.push(opt)
                                }
                            }
                        }
                        vdevs.push(vdev_config.clone());
                        set.push(InstallInstruction::CreatePool { vdevs, pool_options })
                    }
                    "locale" => {
                        set.push(InstallInstruction::SetLocale { name: options[0].clone(), unicode: true })
                    }
                    "bootenv" | "newbe" => {
                        set.push(InstallInstruction::CreateBootEnvironment(options[0].clone()))
                    }
                    "image" | "install-image" => {
                        set.push(InstallInstruction::InstallImage {
                            url: options[0].clone(),
                            image_options: args.clone(),
                        })
                    }
                    _ => return Err(anyhow!(UnknownInstruction(cmd.clone())))
                }
            }
            Config::EOI => {}
            _ => return Err(anyhow!(BadConfigParsed))
        }
    }

    Ok(set)
}