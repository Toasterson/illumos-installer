extern crate pest;
#[macro_use]
extern crate pest_derive;

use anyhow::{anyhow, Result};
use pest::iterators::Pairs;
use pest::Parser;

#[allow(dead_code)]
pub static SHADOW_FILE: &str = "/etc/shadow";

#[derive(Debug, Clone)]
pub struct ShadowEntry {
    username: String,
    password_hash: String,
    password_locked: bool,
    no_login: bool,
    no_password: bool,
    password_last_changed: i64,
    min: i64,
    max: i64,
    warn: i64,
    inactive: i64,
    expire: i64,
    flag: i64,
}

impl Default for ShadowEntry {
    fn default() -> Self {
        ShadowEntry {
            username: String::new(),
            password_hash: String::new(),
            password_locked: false,
            no_login: false,
            no_password: false,
            password_last_changed: 0,
            min: -1,
            max: -1,
            warn: -1,
            inactive: -1,
            expire: -1,
            flag: 0,
        }
    }
}

impl ShadowEntry {
    /// Update the entries password hash in a safe way
    /// (meaning use a good cryptographic algorithm)
    pub fn update_password_hash(&mut self, clear_new_password: &str) -> Result<()> {
        use pwhash::sha512_crypt::hash;
        self.password_hash = hash(clear_new_password)?;
        Ok(())
    }

    pub fn set_password_hash(&mut self, new_hash: &str) {
        self.password_hash = new_hash.clone().into()
    }

    /// Use this function to check if the hash of the entry
    /// has the password you think it does
    /// pass the cleartext password to check the entries hash against
    /// as argument
    pub fn check_password(&self, password: &str) -> Result<()> {
        use pwhash::unix::verify;
        if verify(password, &self.password_hash) {
            return Ok(());
        }

        Err(anyhow!("Passwords do not match"))
    }

    fn print_password_entry(&self) -> String {
        if self.password_locked {
            String::from("*LK*")
        } else if self.no_login {
            String::from("NL")
        } else if self.no_password {
            String::from("NP")
        } else {
            self.password_hash.clone()
        }
    }

    fn print_lastchg(&self) -> String {
        if self.password_last_changed != 0 {
            format!("{}", self.password_last_changed)
        } else {
            String::new()
        }
    }

    fn print_min(&self) -> String {
        if self.min != -1 {
            format!("{}", self.min)
        } else {
            String::new()
        }
    }

    fn print_max(&self) -> String {
        if self.max != -1 {
            format!("{}", self.max)
        } else {
            String::new()
        }
    }

    fn print_warn(&self) -> String {
        if self.warn != -1 {
            format!("{}", self.warn)
        } else {
            String::new()
        }
    }

    fn print_inactive(&self) -> String {
        if self.inactive != -1 {
            format!("{}", self.inactive)
        } else {
            String::new()
        }
    }

    fn print_expire(&self) -> String {
        if self.expire != -1 {
            format!("{}", self.expire)
        } else {
            String::new()
        }
    }

    fn print_flag(&self) -> String {
        if self.flag != 0 {
            format!("{}", self.flag)
        } else {
            String::new()
        }
    }
}

#[derive(Debug, Default)]
pub struct ShadowFile {
    entries: Vec<ShadowEntry>,
}

impl ShadowFile {
    /// Get the shadow entry with `username` as username
    pub fn get_entry(&self, username: &str) -> Option<ShadowEntry> {
        for e in &self.entries {
            if e.username.as_str() == username {
                return Some(e.clone());
            }
        }

        None
    }

    /// Update the shadow entry if one with the same
    /// username already exists or insert a new one at the end
    pub fn insert_or_update(&mut self, entry: ShadowEntry) {
        for (i, e) in self.entries.iter().enumerate() {
            if e.username == entry.username {
                self.entries[i] = entry.clone();
                return;
            }
        }
    }

    /// This function writes the Shadow Entry in the format expected by
    /// /etc/shadow
    pub fn serialize(&self) -> String {
        let mut file = String::new();

        for (i, e) in self.entries.iter().enumerate() {
            if i > 0 {
                file += "\n"
            }

            file += &format!(
                "{}:{}:{}:{}:{}:{}:{}:{}:{}",
                e.username,
                e.print_password_entry(),
                e.print_lastchg(),
                e.print_min(),
                e.print_max(),
                e.print_warn(),
                e.print_inactive(),
                e.print_expire(),
                e.print_flag()
            )
        }

        file
    }
}

/// This function provides a safe default to generate a password hash for
/// /etc/shadow files. Use this to prehash a password in the configuration
/// ```no_run
/// use libshadow::gen_password_hash;
///
/// let hash = gen_password_hash("clear_password").unwrap();
/// // Do something with Hash
/// ```
pub fn gen_password_hash(clear_password: &str) -> Result<String> {
    use pwhash::sha512_crypt::hash;
    Ok(hash(clear_password)?)
}

#[derive(Parser)]
#[grammar = "shadow.pest"]
struct ShadowParser;

/// Parse a Shadow file and get the entries in easily
/// changeable form
/// ```no_run
/// use libshadow::{parse_shadow_file, SHADOW_FILE};
/// use std::fs;
/// let contents = fs::read_to_string(SHADOW_FILE).unwrap();
///
/// let shadow = parse_shadow_file(&contents).unwrap();
/// // Do something with the file
/// let new_file = shadow.serialize();
/// // Do something with the new file
/// ```
pub fn parse_shadow_file(file: &str) -> Result<ShadowFile> {
    let shadow_file: Pairs<Rule> = ShadowParser::parse(Rule::shadow_file, file)?;

    let mut shadow_file_struct = ShadowFile::default();

    for pair in shadow_file {
        for file_pair in pair.into_inner() {
            let mut shadow_entry = ShadowEntry::default();
            for entry_pair in file_pair.into_inner() {
                match entry_pair.as_rule() {
                    Rule::username => shadow_entry.username = entry_pair.as_str().into(),
                    Rule::password => shadow_entry.password_hash = entry_pair.as_str().into(),
                    Rule::no_login => shadow_entry.no_login = true,
                    Rule::no_password => shadow_entry.no_password = true,
                    Rule::locked_password => shadow_entry.password_locked = true,
                    Rule::lastchg => {
                        shadow_entry.password_last_changed = entry_pair.as_str().parse::<i64>()?;
                    }
                    Rule::min => {
                        shadow_entry.min = entry_pair.as_str().parse::<i64>()?;
                    }
                    Rule::max => {
                        shadow_entry.max = entry_pair.as_str().parse::<i64>()?;
                    }
                    Rule::warn => {
                        shadow_entry.warn = entry_pair.as_str().parse::<i64>()?;
                    }
                    Rule::inactive => {
                        shadow_entry.inactive = entry_pair.as_str().parse::<i64>()?;
                    }
                    Rule::expire => {
                        shadow_entry.expire = entry_pair.as_str().parse::<i64>()?;
                    }
                    Rule::flag => {
                        shadow_entry.flag = entry_pair.as_str().parse::<i64>()?;
                    }
                    _ => {}
                }
            }
            if shadow_entry.username != "" {
                shadow_file_struct.entries.push(shadow_entry)
            }
        }
    }

    Ok(shadow_file_struct)
}

#[cfg(test)]
mod tests {
    use crate::parse_shadow_file;

    static EXAMPLE_SHADOW: &str = r#"root:$6$L2Yjwxe3zlIDk4yf$1RwTeVJL2erBYnyIVerOlN5/aoyELMyquctogNESxd/gZQ11mzh4NM5QS6.S.CIslv4LzRYZ1sqVDEqBKTKvv1:6445::::::
daemon:NP:6445::::::
bin:NP:6445::::::
sys:NP:6445::::::
adm:NP:6445::::::
lp:NP:6445::::::
uucp:NP:6445::::::
nuucp:NP:6445::::::
dladm:*LK*:18675::::::
netadm:*LK*:18675::::::
netcfg:*LK*:18675::::::
smmsp:NP:18675::::::
listen:*LK*:::::::
gdm:*LK*:::::::
zfssnap:NP:::::::
upnp:NP:::::::
xvm:*LK*:6445::::::
mysql:NP:::::::
openldap:*LK*:18675::::::
webservd:*LK*:::::::
svctag:*LK*:6445::::::
unknown:*LK*:18675::::::
nobody:*LK*:6445::::::
noaccess:*LK*:6445::::::
nobody4:*LK*:6445::::::
ftp:*LK*:18675::::::
sshd:*LK*:18675::::::
named:NP:18675::::::
pkg5srv:NP:18675::::::
jenkins:NL:18676::::::"#;

    #[test]
    fn parse_example() {
        let shadow_file = parse_shadow_file(EXAMPLE_SHADOW).unwrap();
        let serialized = shadow_file.serialize();
        assert_eq!(EXAMPLE_SHADOW, serialized)
    }

    #[test]
    fn root_modify_example() {
        let mut shadow_file = parse_shadow_file(EXAMPLE_SHADOW).unwrap();
        let mut root_entry = shadow_file.get_entry("root").unwrap();
        root_entry.update_password_hash("blubber").unwrap();
        root_entry.check_password("blubber").unwrap();
        shadow_file.insert_or_update(root_entry);
        let serialized = shadow_file.serialize();
        assert_ne!(EXAMPLE_SHADOW, serialized)
    }
}
