use anyhow::{anyhow, Result};
use libcfgparser::Keyword;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

pub type InstructionsSet = Vec<Instruction>;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Instruction {
    CreatePool {
        vdevs: Vec<VDEVConfiguration>,
        pool_options: Vec<(String, String)>,
    },
    CreateBootEnvironment(String),
    InstallImage {
        url: String,
        image_options: Option<HashMap<String, String>>,
    },
    CreateDataset {
        name: String,
        mount_options: Option<HashMap<String, String>>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VDEVType {
    Empty,
    Mirror,
    RaidZ1,
    RaidZ2,
    RaidZ3,
}

impl Default for VDEVType {
    fn default() -> Self {
        Self::Empty
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct VDEVConfiguration {
    pub vdev_type: VDEVType,
    pub devices: Vec<String>,
}

#[derive(Error, Debug)]
enum InstructionError {
    #[error("keyword {0} is not known")]
    UnknownInstruction(String),
}

pub fn parse_keywords(keywords: Vec<Keyword>) -> Result<InstructionsSet> {
    let mut set = InstructionsSet::new();
    for c in keywords {
        match c.name.as_str() {
            "zpool-create" => {
                let mut vdevs: Vec<VDEVConfiguration> = vec![];
                let mut pool_options: Vec<(String, String)> = vec![];
                if let Some(args) = c.options {
                    for (name, value) in args {
                        pool_options.push((name, value))
                    }
                }

                let mut vdev_config = VDEVConfiguration::default();
                for opt in c.arguments {
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
                        _ => vdev_config.devices.push(opt),
                    }
                }
                vdevs.push(vdev_config.clone());
                set.push(Instruction::CreatePool {
                    vdevs,
                    pool_options,
                });
            }
            "bootenv" | "newbe" => {
                set.push(Instruction::CreateBootEnvironment(c.arguments[0].clone()));
            }
            "image" | "install-image" => {
                set.push(Instruction::InstallImage {
                    url: c.arguments[0].clone(),
                    image_options: c.options.clone(),
                });
            }
            "ds" | "dataset" => {
                set.push(Instruction::CreateDataset {
                    name: c.arguments[0].clone(),
                    mount_options: c.options.clone(),
                });
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

#[cfg(test)]
mod tests {
    use crate::{Instruction, InstructionsSet, VDEVConfiguration, VDEVType};

    #[test]
    fn serialisation_test() {
        let config_ast: InstructionsSet = vec![
            Instruction::CreatePool {
                vdevs: vec![VDEVConfiguration {
                    vdev_type: VDEVType::Mirror,
                    devices: vec!["c1t0d0s0".into(), "c2t0d0s0".into(), "c3t0d0s0".into()],
                }],
                pool_options: vec![("ashift".into(), "12".into())],
            },
            Instruction::CreateDataset {
                name: "rpool/test".to_string(),
                mount_options: None,
            },
        ];
        let serialized = serde_json::to_string(&config_ast).unwrap();
        println!("serialized = {}", serialized);

        let deserialized: InstructionsSet = serde_json::from_str(&serialized).unwrap();
        println!("deserialized = {:?}", deserialized);
    }
}
