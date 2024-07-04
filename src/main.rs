#[allow(unused_imports)]
use std::env;
use std::{
    fs::{DirBuilder, File},
    io::{BufRead, BufReader, Read, Write},
};

use sha1::{Digest, Sha1};

use clap::{Parser, Subcommand};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};

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
    HashObject {
        #[clap(short = 'w')]
        write: bool,
        filename: String,
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

                let decoder = ZlibDecoder::new(file);
                let mut decoder = BufReader::new(decoder);

                // `<blob|commit|tag|tree> <size>\0<content>`
                // Note: delimiter is captured
                decoder.read_until(0, &mut buf).unwrap();

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
                decoder.read_to_end(&mut buf).unwrap();

                let content = String::from_utf8(buf).unwrap();
                print!("{}", content);
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    std::format!("No such file or directory: {}", &path),
                ));
            }
        }
        Command::HashObject { write, filename } => {
            if let Ok(input_file) = File::open(&filename) {
                let mut content = Vec::new();
                let mut reader = BufReader::new(input_file);
                reader.read_to_end(&mut content).unwrap();

                let mut hasher = Sha1::new();
                hasher.write_all(b"blob ").unwrap();
                hasher
                    .write_all(content.len().to_string().as_bytes())
                    .unwrap();
                hasher.write_all(b"\0").unwrap();
                hasher.write_all(content.as_slice()).unwrap();
                let sha = hasher.finalize();
                let sha = format!("{:x}", sha);

                let dirname = &sha[0..2];
                let filename = &sha[2..];

                if write {
                    DirBuilder::new()
                        .create(format!(".git/objects/{dirname}"))
                        .unwrap();

                    let output_file =
                        File::create(format!(".git/objects/{dirname}/{filename}")).unwrap();

                    let mut encoder = ZlibEncoder::new(output_file, Compression::best());

                    encoder.write_all(b"blob ").unwrap();
                    encoder
                        .write_all(content.len().to_string().as_bytes())
                        .unwrap();
                    encoder.write_all(b"\0").unwrap();
                    encoder.write_all(content.as_slice()).unwrap();
                }

                println!("{}", sha);
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    std::format!("No such file or directory: {}", &filename),
                ));
            }
        }
    }

    return Ok(());
}
