use anyhow::Result;
use clap::{ArgEnum, Parser, Subcommand};
use ron::ser::PrettyConfig;
use serde::Serialize;
use shrust::{Shell, ShellIO};
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{stdout, Write};
use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[clap(subcommand)]
    commands: Option<Commands>,

    #[clap(short = 'O', long, env, default_value = "ron")]
    output_format: OutputFormat,
}

#[derive(Subcommand)]
enum Commands {
    Build {
        // Output file for the serialized configuration
        #[clap(short, long, env)]
        output_file: Option<PathBuf>,

        // File to read the human readable config from
        file: PathBuf,
    },
}

#[derive(Error, Debug)]
struct InvalidOutputFormatError {
    format: String,
}

impl Display for InvalidOutputFormatError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "format: {} not know to sysconfigen", self.format)
    }
}

#[derive(ArgEnum, Clone)]
enum OutputFormat {
    JSON,
    JsonPretty,
    YAML,
    RON,
    RonPretty,
}

impl FromStr for OutputFormat {
    type Err = InvalidOutputFormatError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "json" => Ok(Self::JSON),
            "yaml" => Ok(Self::YAML),
            "json-pretty" => Ok(Self::JsonPretty),
            "ron" => Ok(Self::RON),
            "ron-pretty" => Ok(Self::RonPretty),
            _ => Err(InvalidOutputFormatError {
                format: s.clone().to_string(),
            }),
        }
    }
}

impl Display for OutputFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::JSON => write!(f, "json"),
            OutputFormat::YAML => write!(f, "yaml"),
            OutputFormat::JsonPretty => write!(f, "json-pretty"),
            OutputFormat::RON => write!(f, "ron"),
            OutputFormat::RonPretty => write!(f, "ron-pretty"),
        }
    }
}

fn main() -> Result<()> {
    let cli: Cli = Cli::parse();

    if let Some(cmd) = cli.commands {
        match cmd {
            Commands::Build { output_file, file } => {
                let mut out: Box<dyn Write> = if let Some(file) = output_file {
                    Box::new(File::create(file)?)
                } else {
                    Box::new(stdout())
                };
                let mut parser = libcfgparser::SysConfigParser::default();
                for (key, v) in libsysconfig::get_supported_keywords() {
                    parser.add_keyword(key, v);
                }

                let keywords = parser.parse_config_file(file)?;
                let instructions = libsysconfig::parse_keywords(keywords)?;

                match cli.output_format {
                    OutputFormat::JSON => {
                        serde_json::to_writer(&mut out, &instructions)?;
                    }
                    OutputFormat::YAML => {
                        serde_yaml::to_writer(&mut out, &instructions)?;
                    }
                    OutputFormat::JsonPretty => {
                        serde_json::to_writer_pretty(&mut out, &instructions)?;
                    }
                    OutputFormat::RON => {
                        let mut ser = ron::Serializer::new(&mut out, None, true)?;
                        instructions.serialize(&mut ser)?;
                    }
                    OutputFormat::RonPretty => {
                        let mut ser =
                            ron::Serializer::new(&mut out, Some(PrettyConfig::default()), true)?;
                        instructions.serialize(&mut ser)?;
                    }
                }
            }
        }
    } else {
        let v = Vec::new();
        let mut shell = Shell::new(v);
        shell.new_command("push", "Add string to the list", 1, |io, v, s| {
            writeln!(io, "Pushing {}", s[0])?;
            v.push(s[0].to_string());
            Ok(())
        });
        shell.new_command_noargs("list", "List strings", |io, v| {
            for s in v {
                writeln!(io, "{}", s)?;
            }
            Ok(())
        });

        shell.run_loop(&mut ShellIO::default());
    }

    Ok(())
}
