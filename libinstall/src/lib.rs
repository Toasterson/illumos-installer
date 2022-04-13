extern crate pest;
#[macro_use]
extern crate pest_derive;

pub mod config;

#[cfg(test)]
mod tests {
    use crate::config;
    use crate::config::parse_config_to_instructions;

    #[test]
    fn initial_test() {
        let config_file = "zpool-create --ashift=\"12\" mirror c1t0d0s0 c2t0d0s0 c3t0d0s0\nlocale en_US\n";
        let config_ast = config::parse_config(config_file).unwrap();
        let config_instructions = parse_config_to_instructions(config_ast);
        println!("{:?}", config_instructions);
    }
}
