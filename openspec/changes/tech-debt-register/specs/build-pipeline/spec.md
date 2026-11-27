## ADDED Requirements

### Requirement: Tech-debt register check
A portable check (`cargo xtask`, run by the pre-commit hook and CI) SHALL fail when:
- an entry in `docs/tech-debt/` has invalid front matter, an unknown severity, a body without a fix, or a path that does not exist;
- two entries share an ID;
- `NEXT_ID` is not above every ID.

A lookup SHALL list the entries whose paths match given files, the files changed since a revision, or the paths an OpenSpec change names. For a change, it SHALL fail while a matching entry is not mentioned by the change.

#### Scenario: Entry names a removed file
- **WHEN** a change deletes or renames a file that an entry lists in its paths
- **THEN** the check fails and names the entry and the path

#### Scenario: Two branches take the same ID
- **WHEN** two branches each add `TD-132.md` with different content and are merged
- **THEN** git reports a conflict on `NEXT_ID` or on the entry file, and the check fails until each entry has its own ID

#### Scenario: Lookup for a changed file
- **WHEN** an entry lists `crates/df-db/src/` and a branch changes `crates/df-db/src/runs/tests.rs`
- **THEN** the lookup with `--since main` prints that entry's ID, title and the matching path

#### Scenario: Change that does not address an entry
- **WHEN** an OpenSpec change names `crates/factory-agent/src/tests/connection.rs` in an artifact, an entry lists that file, and no artifact of the change mentions the entry's ID
- **THEN** the lookup with `--change` prints the entry and fails

#### Scenario: Change that addresses an entry
- **WHEN** the same change names the entry's ID in its proposal, as fixed or as out of scope
- **THEN** the lookup with `--change` succeeds

#### Scenario: Lookup without a hit
- **WHEN** no entry lists a changed file or a directory above it
- **THEN** the lookup prints nothing and succeeds
