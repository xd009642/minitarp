use clap::{crate_version, App};
use minitarp::prelude::*;

fn main() -> Result<(), Error> {
    let args = App::new("minitarp")
        .author("Daniel McKenna, <danielmckenna93@gmail.com>")
        .about("Debugging tool for cargo-tarpaulin")
        .version(concat!("version: ", crate_version!()))
        .args_from_usage("--data -d [TOML] 'link to a minitarp config file'")
        .get_matches();

    let config = args.value_of("data").unwrap_or_else(|| "minitarp.toml");

    if let Ok(conf) = std::fs::read_to_string(config) {
        let config: Config =
            toml::from_str(&conf).map_err(|e| Error::BadToml(format!("Invalid toml {}", e)))?;
        println!("Running for {} on breakpoints:", config.binary.display());
        for b in &config.breakpoints {
            print!("{:x} ", b);
        }
        run(&config)?;
    }
    Ok(())
}
