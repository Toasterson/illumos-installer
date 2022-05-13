use anyhow::{anyhow, Result};
use clap::Parser;
use libshadow::{gen_password_hash, parse_shadow_file, SHADOW_FILE};
use std::fs;

/// shadow file modification utility
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Output in json format
    #[clap(long)]
    json: bool,

    /// shadow entry to modify
    #[clap(short, long)]
    shadow: Option<String>,

    /// Print the passwords Hash
    #[clap(short = 'H', long)]
    hash: Option<String>,

    /// Alternate shadow file to modify
    #[clap(short, long)]
    file: Option<String>,

    input: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if let Some(hash) = args.hash {
        let hash = gen_password_hash(&hash)?;
        println!("{}", hash);
        return Ok(());
    }

    if let Some(shadow_name) = args.shadow {
        if args.input.is_none() {
            return Err(anyhow!(
                "Please provide password or hash as positional argument"
            ));
        }

        let contents = if let Some(file) = args.file {
            fs::read_to_string(file)?
        } else {
            fs::read_to_string(SHADOW_FILE)?
        };
        let mut shadow = parse_shadow_file(&contents)?;
        let account_name: String = shadow_name;
        let pw_or_hash: String = args.input.unwrap();

        if let Some(mut entry) = shadow.get_entry(&account_name) {
            // Assume it's a Hash if input starts with $
            if pw_or_hash.starts_with("$") {
                entry.set_password_hash(&pw_or_hash)
            } else {
                entry.update_password_hash(&pw_or_hash)?;
            }
            shadow.insert_or_update(entry);
            let new_file = shadow.serialize();

            println!("{}", new_file)
        } else {
            return Err(anyhow!("No entry named {} in shadow file", account_name));
        }

        return Ok(());
    }

    Err(anyhow!("Please either specify -H or -s for modes"))
}
