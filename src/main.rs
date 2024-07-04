#[allow(unused_imports)]
use std::env;
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Write},
};

use anyhow::{anyhow, Context};
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

#[derive(Eq, PartialEq)]
enum ObjectType {
    Blob,
}

struct GitObject {
    object_type: ObjectType,
    _size: usize,
    data: Vec<u8>,
}

// `<blob> <size>\0<content>`.
fn parse_git_object(mut reader: BufReader<ZlibDecoder<File>>) -> Result<GitObject, anyhow::Error> {
    let mut buf = Vec::new();

    reader.read_until(b' ', &mut buf)?;

    let object_type = if buf.starts_with(b"blob") {
        ObjectType::Blob
    } else {
        let object_type = std::str::from_utf8(&buf).context("not utf8 ?")?;
        let object_type = &object_type[0..object_type.len() - 1];

        return Err(anyhow!("Object type `{object_type}` is not supported"));
    };

    buf.clear();
    reader.read_until(0, &mut buf)?;

    let size = std::str::from_utf8(&buf)
        .context("not utf8 ?")?
        .parse::<usize>()
        .context("not a number ?")?;

    buf.clear();
    let n = reader.read_to_end(&mut buf)?;

    if size != n {
        anyhow::bail!("Expected {size} bytes, got {n}");
    }

    return Ok(GitObject {
        object_type,
        _size: size,
        data: buf,
    });
}

fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();

    match args.command {
        Command::Init => {
            fs::create_dir(".git")?;
            fs::create_dir(".git/objects")?;
            fs::create_dir(".git/refs")?;
            fs::write(".git/HEAD", "ref: refs/heads/main\n")?;
            println!("Initialized git directory")
        }
        Command::CatFile {
            pretty_print,
            object_hash,
        } => {
            anyhow::ensure!(pretty_print, "expected pretty print (-p)");

            let dirname = &object_hash[0..2];
            let filename = &object_hash[2..];
            let path = std::format!(".git/objects/{dirname}/{filename}");

            match File::open(&path) {
                Ok(file) => {
                    let decoder = ZlibDecoder::new(file);
                    let reader = BufReader::new(decoder);
                    let object = parse_git_object(reader)?;

                    anyhow::ensure!(
                        object.object_type == ObjectType::Blob,
                        "cat-file can only read blob objects"
                    );

                    print!("{}", std::str::from_utf8(&object.data)?);
                }
                Err(_) => {
                    anyhow::bail!("No such file or directory: {}", &path);
                }
            }
        }
        Command::HashObject { write, filename } => match File::open(&filename) {
            Ok(input_file) => {
                let mut content = Vec::new();
                let mut reader = BufReader::new(input_file);
                reader.read_to_end(&mut content).unwrap();

                let object = [
                    b"blob ",
                    content.len().to_string().as_bytes(),
                    b"\0",
                    content.as_slice(),
                ]
                .concat();

                let mut hasher = Sha1::new();
                hasher.write_all(&object)?;

                let sha = hasher.finalize();
                let object_hash = format!("{:x}", sha);

                let dirname = &object_hash[0..2];
                let filename = &object_hash[2..];

                if write {
                    fs::create_dir(format!(".git/objects/{dirname}"))?;
                    let output_file = File::create(format!(".git/objects/{dirname}/{filename}"))?;
                    let mut encoder = ZlibEncoder::new(output_file, Compression::best());

                    encoder.write(&object)?;
                }

                println!("{}", object_hash);
            }
            Err(_) => {
                anyhow::bail!("No such file or directory: {}", &filename);
            }
        },
    }

    return Ok(());
}
