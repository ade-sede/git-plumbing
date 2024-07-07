#[allow(unused_imports)]
use std::env;
use std::{
    cmp::Ordering,
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::MetadataExt,
    path::PathBuf,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
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
    WriteTree,
    CommitTree {
        #[clap(short = 'm')]
        commit_message: String,

        #[clap(short = 'p')]
        parent_hash: String,

        tree_hash: String,
    },
}

#[allow(unused_imports)]
use std::fs;

#[derive(Eq, PartialEq, Debug)]
enum ObjectType {
    Blob,
    Tree,
    Commit,
}

struct BlobObject {
    data: Vec<u8>,
}

impl BlobObject {
    pub fn pack(self: &BlobObject) -> Vec<u8> {
        return [
            b"blob ",
            self.data.len().to_string().as_bytes(),
            b"\0",
            self.data.as_slice(),
        ]
        .concat();
    }
}

#[derive(Clone, Debug, Eq)]
struct TreeEntry {
    mode: u32,
    name: String,
    sha: String,
}

impl TreeEntry {
    pub fn pack(self: &TreeEntry) -> Vec<u8> {
        let sha = hex::decode(&self.sha).unwrap();

        return [
            self.mode.to_string().as_bytes(),
            b" ",
            self.name.to_string().as_bytes(),
            b"\0",
            sha.as_slice(),
        ]
        .concat();
    }
}

// git is very particular about how it sorts entries in a tree
// 1. case-sensitive (uppercase before lowercase)
// 2. for the sake of comparison, directories are treated as if there were a trailing `/`
impl PartialOrd for TreeEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TreeEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_name = if self.mode == 040000 {
            format!("{}/", self.name)
        } else {
            self.name.clone()
        };
        let other_name = if other.mode == 040000 {
            format!("{}/", other.name)
        } else {
            other.name.clone()
        };

        self_name.cmp(&other_name)
    }
}

impl PartialEq for TreeEntry {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.mode == other.mode
    }
}

struct TreeObject {
    entries: Vec<TreeEntry>,
}

impl TreeObject {
    pub fn pack(self: &mut TreeObject) -> Vec<u8> {
        self.entries.sort();

        let packed = self
            .entries
            .as_slice()
            .into_iter()
            .map(|entry| entry.pack())
            .collect::<Vec<Vec<u8>>>()
            .concat();

        return [
            b"tree ",
            packed.len().to_string().as_bytes(),
            b"\0",
            packed.as_slice(),
        ]
        .concat();
    }
}

struct CommitObject {
    tree_hash: String,
    parents: Vec<String>,
    author_name: String,
    author_email: String,
    author_date_seconds: SystemTime,
    author_date_timezone: String,
    committer_name: String,
    committer_email: String,
    committer_date_seconds: SystemTime,
    committer_date_timezone: String,
    commit_message: String,
}

impl CommitObject {
    pub fn pack(self: &CommitObject) -> Result<Vec<u8>, anyhow::Error> {
        let tree_hash = [b"tree ", self.tree_hash.as_bytes(), b"\n"].concat();
        let parents = self
            .parents
            .as_slice()
            .into_iter()
            .map(|parent_hash| [b"parent ", parent_hash.as_bytes(), b"\n"].concat())
            .collect::<Vec<Vec<u8>>>()
            .concat();

        let author = [
            b"author ",
            self.author_name.as_bytes(),
            b" ",
            self.author_email.as_bytes(),
            b" ",
            self.author_date_seconds
                .duration_since(UNIX_EPOCH)?
                .as_secs()
                .to_string()
                .as_bytes(),
            b" ",
            self.author_date_timezone.as_bytes(),
            b"\n",
        ]
        .concat();

        let committer = [
            b"committer ",
            self.committer_name.as_bytes(),
            b" ",
            self.committer_email.as_bytes(),
            b" ",
            self.committer_date_seconds
                .duration_since(UNIX_EPOCH)?
                .as_secs()
                .to_string()
                .as_bytes(),
            b" ",
            self.committer_date_timezone.as_bytes(),
            b"\n",
        ]
        .concat();

        let message = [b"\n", self.commit_message.as_bytes(), b"\n"].concat();

        let content = [tree_hash, parents, author, committer, message].concat();

        let packed = [
            b"commit ",
            content.len().to_string().as_bytes(),
            b"\0",
            content.as_slice(),
        ]
        .concat();

        return Ok(packed);
    }
}

enum GitObject {
    Blob(BlobObject),
    Tree(TreeObject),
    #[allow(dead_code)]
    Commit(CommitObject),
}

fn read_tree_entry(
    reader: &mut BufReader<ZlibDecoder<File>>,
) -> Result<(TreeEntry, usize), anyhow::Error> {
    let mut buf = Vec::new();
    let mut total = 0;

    let n = reader.read_until(b' ', &mut buf)?;
    total += n;

    let mode = buf.strip_suffix(&[b' ']).unwrap();

    let mode: u32 = std::str::from_utf8(mode)
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
            mode,
            name: String::from_str(std::str::from_utf8(name)?)?,
            sha: hex::encode(sha),
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
    } else if object_type.starts_with(b"comit") {
        ObjectType::Commit
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

            let object = GitObject::Tree(TreeObject { entries });

            return Ok(object);
        }
        ObjectType::Blob => {
            buf.clear();
            let n = reader.read_to_end(&mut buf)?;

            anyhow::ensure!(n == size, "Expected {size} bytes, got {n} bytes");

            let object = GitObject::Blob(BlobObject { data: buf });

            return Ok(object);
        }
        ObjectType::Commit => {
            anyhow::bail!("Unimplemented read: commit");
        }
    };
}

fn hash_object(filename: PathBuf) -> Result<String, anyhow::Error> {
    match File::open(&filename) {
        Ok(input_file) => {
            let mut content = Vec::new();
            let mut reader = BufReader::new(input_file);
            reader.read_to_end(&mut content).unwrap();

            let object = BlobObject {
                data: content.clone(),
            };

            let packed_object = object.pack();
            let object_hash = write_object_file(packed_object)?;

            return Ok(object_hash);
        }
        Err(_) => {
            anyhow::bail!("No such file or directory: {:?}", filename);
        }
    }
}

fn write_tree(path: PathBuf) -> Result<String, anyhow::Error> {
    let directory = fs::read_dir(path)?;
    let mut entries: Vec<TreeEntry> = Vec::new();

    for entry in directory {
        if let Ok(entry) = entry {
            let file_name = String::from_str(entry.file_name().to_str().unwrap())?;
            let file_type = entry.file_type()?;

            let sha = if file_type.is_file() {
                hash_object(entry.path())?
            } else if file_type.is_dir() {
                if file_name == ".git" {
                    continue;
                }
                write_tree(entry.path())?
            } else {
                anyhow::bail!("Neither file nor dir");
            };

            // Took this small snippet to compute mode from johnoo's implementation.
            // I'm not entirely sure i understand git object mode.
            // Seems like mode is a mix of
            // - file type (first 3 digits)
            // - unix permissions (last 3 digits)
            //
            // 040 -> dir
            // 120 -> symlink
            // 100 -> normal file
            // 160 -> submodule (not covered here)
            let mode = if file_type.is_dir() {
                040000
            } else if file_type.is_symlink() {
                120000
            } else if (entry.metadata()?.mode() & 0o111) != 0 {
                // has at least one executable bit set
                100755
            } else {
                100644
            };

            entries.push(TreeEntry {
                mode,
                name: file_name,
                sha,
            })
        }
    }

    let mut tree = TreeObject { entries };
    let packed_tree = tree.pack();
    let tree_hash = write_object_file(packed_tree)?;

    return Ok(tree_hash);
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
        Command::HashObject { write, filename } => {
            anyhow::ensure!(write, "expected -w");

            let hash = hash_object(PathBuf::from(filename))?;
            print!("{hash}");
        }
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
        Command::WriteTree => {
            // Assume that every file in the current directory needs to be convered to git objects.
            // Usually, only files / directories in the staging area need to be converted to git
            // objects.
            let tree_hash = write_tree(PathBuf::from("."))?;
            print!("{tree_hash}");
        }
        Command::CommitTree {
            tree_hash,
            parent_hash,
            commit_message,
        } => {
            let commit = CommitObject {
                tree_hash,
                commit_message,
                parents: vec![parent_hash],
                author_date_seconds: SystemTime::now(),
                author_date_timezone: "+0001".to_string(),
                author_email: "bogus-mail@bogus-exchange.com".to_string(),
                author_name: "A Koala".to_string(),
                committer_date_seconds: SystemTime::now(),
                committer_date_timezone: "+0001".to_string(),
                committer_email: "bogus-mail@bogus-exchange.com".to_string(),
                committer_name: "A Koala".to_string(),
            };

            let packed_commit = commit.pack()?;
            let commit_hash = write_object_file(packed_commit)?;

            print!("{commit_hash}");
        }
    }

    return Ok(());
}

fn write_object_file(packed: Vec<u8>) -> Result<String, anyhow::Error> {
    let mut hasher = Sha1::new();
    hasher.write_all(&packed)?;

    let sha = hasher.finalize();
    let hash = hex::encode(sha);

    let dirname = &hash[0..2];
    let filename = &hash[2..];

    fs::create_dir(format!(".git/objects/{dirname}"))?;
    let output_file = File::create(format!(".git/objects/{dirname}/{filename}"))?;
    let mut encoder = ZlibEncoder::new(output_file, Compression::best());

    encoder.write(&packed)?;

    return Ok(hash);
}
