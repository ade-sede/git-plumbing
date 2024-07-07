["Build Your Own Git" Challenge](https://codecrafters.io/challenges/git).

Learn about git objects and [plumbing commands](https://git-scm.com/book/en/v2/Git-Internals-Plumbing-and-Porcelain).

```bash
# Initialize a git directory
$> git init

# Read a blob object
$> cargo run cat-file -p <blob_sha>

# Create a blob object
$> cargo run hash-object -w </path/to/file/in/repo

# Read a tree object
$> cargo run ls-tree --name-only <tree_sha>

# Write a tree object (corresponding to all files in current directly, recursively)
$> cargo run write-tree

# Create a commit object
$> cargo run commit-tree <tree_sha> -p <commit_sha> -m <message>
```

# TODO

- Git clone
