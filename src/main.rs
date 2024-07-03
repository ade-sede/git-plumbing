#[allow(unused_imports)]
use std::env;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    command_name: String,
}

#[allow(unused_imports)]
use std::fs;

fn main() {
    let args = Args::parse();

    if args.command_name == "init" {
        fs::create_dir(".git").unwrap();
        fs::create_dir(".git/objects").unwrap();
        fs::create_dir(".git/refs").unwrap();
        fs::write(".git/HEAD", "ref: refs/heads/main\n").unwrap();
        println!("Initialized git directory")
    } else {
        println!("unknown command: {}", args.command_name)
    }
}
