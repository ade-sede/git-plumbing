#[allow(unused_imports)]
use std::env;
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    str::FromStr,
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
    LsTree {
        #[clap(long)]
        name_only: bool,
        object_hash: String,
    },
}

#[allow(unused_imports)]
use std::fs;

#[derive(Eq, PartialEq)]
enum ObjectType {
    Blob,
    Tree,
}

struct BlobObject {
    _data_size: usize,
    data: Vec<u8>,
}

struct TreeEntry {
    _mode: u64,
    name: String,
    _sha: Vec<u8>,
}

struct TreeObject {
    _data_size: usize,
    entries: Vec<TreeEntry>,
}

enum GitObject {
    Blob(BlobObject),
    Tree(TreeObject),
}

fn read_tree_entry(
    reader: &mut BufReader<ZlibDecoder<File>>,
) -> Result<(TreeEntry, usize), anyhow::Error> {
    let mut buf = Vec::new();
    let mut total = 0;

    let n = reader.read_until(b' ', &mut buf)?;
    total += n;

    let mode = buf.strip_suffix(&[b' ']).unwrap();

    let mode: u64 = std::str::from_utf8(mode)
        .context("not utf8 ?")?
        .parse()
        .context("not a number ?")?;

    buf.clear();
    let n = reader.read_until(0, &mut buf)?;
    total += n;
    let name = buf.strip_suffix(&[0]).unwrap();

    let mut sha = vec![0u8; 20];
    reader.read_exact(&mut sha)?;
    total += 20;

    return Ok((
        TreeEntry {
            _mode: mode,
            name: String::from_str(std::str::from_utf8(name)?)?,
            _sha: sha,
        },
        total,
    ));
}

// `<blob> <content-size>\0<content>`
// `<tree> <content-size>\0<content>` where `<content>`
//      `<mode> <name>\0<20 bytes sha>`
fn read_git_object(reader: &mut BufReader<ZlibDecoder<File>>) -> Result<GitObject, anyhow::Error> {
    let mut buf = Vec::new();

    reader.read_until(b' ', &mut buf)?;
    let object_type = buf.strip_suffix(&[b' ']).unwrap();

    let object_type = if object_type.starts_with(b"blob") {
        ObjectType::Blob
    } else if object_type.starts_with(b"tree") {
        ObjectType::Tree
    } else {
        let object_type = std::str::from_utf8(&buf).context("not utf8 ?")?;

        return Err(anyhow!("Object type `{object_type}` is not supported"));
    };

    buf.clear();
    reader.read_until(0, &mut buf)?;
    let size = buf.strip_suffix(&[0]).unwrap();

    let size: usize = std::str::from_utf8(size)
        .context("not utf8 ?")?
        .parse()
        .context("not a number ?")?;

    match object_type {
        ObjectType::Tree => {
            let mut entries: Vec<TreeEntry> = Vec::new();
            let mut remaining = size;

            while remaining > 0 {
                let (entry, n) = read_tree_entry(reader)?;
                entries.push(entry);
                remaining -= n;
            }

            let object = GitObject::Tree(TreeObject {
                _data_size: size,
                entries,
            });

            return Ok(object);
        }
        ObjectType::Blob => {
            buf.clear();
            let n = reader.read_to_end(&mut buf)?;

            anyhow::ensure!(n == size, "Expected {size} bytes, got {n} bytes");

            let object = GitObject::Blob(BlobObject {
                _data_size: size,
                data: buf,
            });

            return Ok(object);
        }
    };
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
                    let mut reader = BufReader::new(decoder);
                    let object = read_git_object(&mut reader)?;

                    match object {
                        GitObject::Blob(blob) => {
                            print!("{}", std::str::from_utf8(&blob.data)?);
                        }
                        _ => {
                            anyhow::bail!("cat-file can only read blob objects");
                        }
                    }
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
                let object_hash = hex::encode(sha);

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
        Command::LsTree {
            name_only,
            object_hash,
        } => {
            anyhow::ensure!(name_only, "expected --name-only");

            let dirname = &object_hash[0..2];
            let filename = &object_hash[2..];
            let path = std::format!(".git/objects/{dirname}/{filename}");

            match File::open(&path) {
                Ok(file) => {
                    let decoder = ZlibDecoder::new(file);
                    let mut reader = BufReader::new(decoder);
                    let object = read_git_object(&mut reader)?;

                    match object {
                        GitObject::Tree(tree) => {
                            for entry in tree.entries.into_iter() {
                                println!("{}", entry.name);
                            }
                        }
                        _ => {
                            anyhow::bail!("ls-tree can only read tree objects");
                        }
                    }
                }
                Err(_) => {
                    anyhow::bail!("No such file or directory: {}", &path);
                }
            }
        }
    }

    return Ok(());
}
