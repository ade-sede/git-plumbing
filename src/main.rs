#[allow(unused_imports)]
use std::env;
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
};

use clap::{Parser, Subcommand};
use flate2::read::ZlibDecoder;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Init,
    CatFile {
        #[clap(short = 'p')]
        pretty_print: bool,

        object_hash: String,
    },
}

#[allow(unused_imports)]
use std::fs;

fn main() -> std::io::Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Init => {
            fs::create_dir(".git").unwrap();
            fs::create_dir(".git/objects").unwrap();
            fs::create_dir(".git/refs").unwrap();
            fs::write(".git/HEAD", "ref: refs/heads/main\n").unwrap();
            println!("Initialized git directory")
        }
        Command::CatFile {
            pretty_print,
            object_hash,
        } => {
            if !pretty_print {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "git cat-file: -p option is mandatory",
                ));
            }

            let dirname = &object_hash[0..2];
            let filename = &object_hash[2..];
            let path = std::format!(".git/objects/{dirname}/{filename}");

            if let Ok(file) = File::open(&path) {
                let mut buf = Vec::new();

                let decompressor = ZlibDecoder::new(file);
                let mut decompressor = BufReader::new(decompressor);

                // `<blob|commit|tag|tree> <size>\0<content>`
                // Note: delimiter is captured
                decompressor.read_until(0, &mut buf).unwrap();

                let metadata = &buf[0..buf.len() - 1];
                let mut iter = metadata.split(|&x| x == b' ');

                let object_type = iter.next().unwrap();
                let object_type = std::str::from_utf8(object_type).unwrap();

                let object_size = iter.next().unwrap();
                let object_size = std::str::from_utf8(object_size).unwrap();
                let _object_size = object_size.parse::<usize>().unwrap();

                if object_type != "blob" {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        std::format!("Unsupported object type {}", object_type),
                    ));
                }

                buf.clear();
                decompressor.read_to_end(&mut buf).unwrap();

                let content = String::from_utf8(buf).unwrap();
                print!("{}", content);
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    std::format!("No such file or directory: {}", &path),
                ));
            }
        }
    }

    return Ok(());
}
