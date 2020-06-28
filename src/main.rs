use clap::{crate_version, App};
use std::fs::File;

mod timeline;

use crate::timeline::*;

fn main() {
    let args = App::new("minitarp")
        .author("Daniel McKenna, <danielmckenna93@gmail.com>")
        .about("Debugging tool for cargo-tarpaulin")
        .version(concat!("version: ", crate_version!()))
        .args_from_usage(
            "--input -i <FILE> 'link to a tarpaulin traced output'
                         --output -o [FILE] 'place to save output file'",
        )
        .get_matches();

    let traces = args.value_of("input").expect("Expected an input");
    let output = args
        .value_of("output")
        .unwrap_or_else(|| "tarpaulin_plot.png");

    let fl = File::open(traces).expect("File doesn't exist");

    let log: EventLog = serde_json::from_reader(fl).expect("Failed to parse file");

    log.save_graph(output);
}
