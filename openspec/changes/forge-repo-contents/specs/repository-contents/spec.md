## Purpose

Read a directory listing or a single file of a project's repository at a revision, without a clone, through the project's forge plugin. Consumers are control-plane features such as the image builder and repository configuration.

## ADDED Requirements

### Requirement: Revisions
A read SHALL take a revision that is a branch, a tag or a commit, and SHALL name the commit it read. Reading at a branch or tag SHALL read its current commit. A revision that does not exist SHALL fail with `not-found`.

#### Scenario: Read at a branch
- **WHEN** a file is read at branch `main` whose current commit is `abc123`
- **THEN** the result names commit `abc123`

#### Scenario: Pinned follow-up read
- **WHEN** a directory is listed at branch `main`, then `main` moves, then a file is read at the commit the listing named
- **THEN** the file's content is the one at the listed commit

#### Scenario: Unknown branch
- **WHEN** a file is read at branch `does-not-exist`
- **THEN** the read fails with `not-found`

### Requirement: Directory listing
Listing a directory SHALL return its direct entries only, each with name, path relative to the repository root, kind (`file`, `dir`, `symlink` or `submodule`), its Git object ID and size in bytes for files. An empty path SHALL list the repository root. A path that is a file SHALL fail with `invalid-input`; a missing path with `not-found`.

#### Scenario: Root listing
- **WHEN** the root of a repository with `README.md` and `src/main.rs` is listed
- **THEN** the result has a `file` entry `README.md` with its size and blob SHA and a `dir` entry `src`, and no entry `src/main.rs`

#### Scenario: Changed content changes the entry's SHA
- **WHEN** a directory is listed before and after a commit that changes the content of one of its files
- **THEN** that file's entry has a different blob SHA and every other entry's SHA is unchanged

#### Scenario: Symlink and submodule entries
- **WHEN** a directory containing a symlink and a submodule is listed
- **THEN** they are entries of kind `symlink` and `submodule`

#### Scenario: Missing directory
- **WHEN** the path `nope/` is listed
- **THEN** the read fails with `not-found`

### Requirement: Listing size limit
A listing SHALL return at most 1,000 entries. When the directory has more, the result SHALL say it is truncated.

#### Scenario: Large directory
- **WHEN** a directory with 1,500 entries is listed
- **THEN** the result has 1,000 entries and is marked truncated

#### Scenario: Small directory
- **WHEN** a directory with 3 entries is listed
- **THEN** the result is not marked truncated

### Requirement: File read
Reading a file SHALL return its raw bytes, its size and its blob SHA. A symlink SHALL return its target without following it. A submodule SHALL return a submodule marker without content. A directory path SHALL fail with `invalid-input`; a missing path with `not-found`.

#### Scenario: Binary file
- **WHEN** a PNG file is read
- **THEN** the result is its exact bytes with size and blob SHA

#### Scenario: Symlink not followed
- **WHEN** a symlink `link` pointing at `../secret` is read
- **THEN** the result is the target `../secret` and no file content

#### Scenario: Symlink to a file in the repository
- **WHEN** a symlink `docs/link.md` pointing at `../README.md` is read
- **THEN** the result is the target `../README.md` and not the content of `README.md`

#### Scenario: Symlink to a directory
- **WHEN** a symlink `vendor` pointing at `third_party/` is read
- **THEN** the result is the target `third_party/`

#### Scenario: Directory path
- **WHEN** the path `src` of a directory is read as a file
- **THEN** the read fails with `invalid-input`

### Requirement: File size limit
A file larger than 1 MiB SHALL fail with `too-large` carrying its size, and no content SHALL be returned. The host SHALL reject plugin results whose content exceeds 1 MiB the same way.

#### Scenario: Large file
- **WHEN** a 5 MiB file is read
- **THEN** the read fails with `too-large` and size 5,242,880

#### Scenario: File at the limit
- **WHEN** a file of exactly 1 MiB is read
- **THEN** its content is returned

### Requirement: Project repository only
Reads SHALL be made for a project and SHALL reach only that project's repository.

#### Scenario: Project read
- **WHEN** a file is read for project `DF`
- **THEN** the plugin is called with project `DF`'s repository

### Requirement: Path validation
Paths SHALL be relative to the repository root and `/`-separated. A path with a `..` segment, a leading `/` or a backslash SHALL fail with `invalid-input` before the forge is called.

#### Scenario: Parent segment
- **WHEN** the file `.dark-factory/../secrets.env` is read
- **THEN** the read fails with `invalid-input` and no forge request is made

### Requirement: No path filtering
Reads SHALL return hidden and protected paths like any other path. A consumer that passes content to an agent SHALL filter hidden paths itself.

#### Scenario: Hidden path read
- **WHEN** a file matching a hidden path of the project is read
- **THEN** its content is returned

### Requirement: Forge without contents support
When the project's forge plugin doesn't export `contents`, every read SHALL fail with an error saying the forge doesn't support repository reads, without calling the plugin.

#### Scenario: Plugin without contents
- **WHEN** a file is read for a project whose plugin exports no `contents`
- **THEN** the read fails naming the missing capability
