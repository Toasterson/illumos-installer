extern crate pest;
#[macro_use]
extern crate pest_derive;

use std::collections::HashMap;
use pest::Parser;
use anyhow::{anyhow, Result};
use thiserror::Error;
use serde::{Deserialize, Serialize};

enum Config {
    Instruction(String, Option<Vec<(String, String)>>, Vec<String>),
    Command(String),
    Option(String),
    Argument(String, String),
    Value(String),
    EOI,
}

#[derive(Default, Debug, Deserialize, Serialize)]
pub struct Keyword {
    pub name: String,
    pub options: Option<HashMap<String, String>>,
    pub arguments: Vec<String>
}

#[derive(Default, Debug, Deserialize, Serialize)]
pub struct KeywordDefinition {
    pub options: Vec<String>
}

#[derive(Error, Debug)]
enum ConfigError {
    #[error("cannot build keyword please check that config file is valid")]
    NotInstruction()
}

#[derive(Parser, Default, Debug)]
#[grammar = "config.pest"]
pub struct SysConfigParser {
    keywords: HashMap<String, KeywordDefinition>
}

impl SysConfigParser {
    pub fn add_keyword(&mut self, name: String, k: KeywordDefinition) -> Option<KeywordDefinition> {
        self.keywords.insert(name, k)
    }

    pub fn parse_config(&self, file: &str) -> Result<Vec<Keyword>> {
        let config = SysConfigParser::parse(Rule::config, file)?.next().unwrap();

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

        let mut keywords: Vec<Keyword> = vec![];

        for pair in config.into_inner() {
            match parse_value(pair) {
                Config::Instruction(name, opts, args) => {
                    let opts= if let Some(o) = opts {
                        let mut m = HashMap::<String, String>::new();
                        for (key, value) in o {
                            if !m.contains_key(&key) {
                                m.insert(key, value);
                            }
                        }
                        Some(m)
                    } else {
                        None
                    };
                    // TODO check if Parser has keyword configured
                    // Error if not
                    keywords.push(Keyword{
                        name,
                        options: opts,
                        arguments: args
                    })
                }
                _ => {
                    return Err(anyhow!(ConfigError::NotInstruction()))
                }
            }

        }

        Ok(keywords)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use crate::{KeywordDefinition, SysConfigParser};

    #[test]
    fn initial_test() {
        let config_file = "zpool-create --ashift=\"12\" mirror c1t0d0s0 c2t0d0s0 c3t0d0s0\nlocale en_US\n";
        let mut parser = SysConfigParser::default();
        parser.add_keyword(String::from("zpool-create"), KeywordDefinition{
            options: vec![String::from("ashift")],
        });
        parser.add_keyword(String::from("locale"), KeywordDefinition{
            options: vec![]
        });
        let config_ast = parser.parse_config(config_file).unwrap();
        println!("{:?}", config_ast);
    }
}