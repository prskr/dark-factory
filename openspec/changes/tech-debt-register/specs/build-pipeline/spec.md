## ADDED Requirements

### Requirement: Tech-debt register check
A portable check (`cargo xtask`, run by the pre-commit hook and CI) SHALL fail when an entry in `docs/tech-debt/` has invalid front matter, an unknown severity, a body without a fix, or a path that does not exist, when an entry's file name has a leading zero, and when `NEXT_ID` is not above every ID.

#### Scenario: Entry names a removed file
- **WHEN** a change deletes or renames a file that an entry lists in its paths
- **THEN** the check fails and names the entry and the path

#### Scenario: Leading zero in the file name
- **WHEN** the register holds `TD-05.md`
- **THEN** the check fails and names the file

#### Scenario: Counter not above every ID
- **WHEN** `NEXT_ID` holds 134 and the register holds `TD-134.md`
- **THEN** the check fails and names `NEXT_ID`

### Requirement: Tech-debt lookup
A lookup SHALL list the entries whose paths match given files, the files changed since a revision, or the paths an OpenSpec change names, from the repo root or with the crate name. For a change, it SHALL fail while a matching entry is not mentioned by the change.

#### Scenario: Lookup for a changed file
- **WHEN** an entry lists `crates/df-db/src/` and a branch changes `crates/df-db/src/runs/tests.rs`
- **THEN** the lookup with `--since main` prints that entry's ID, title and the matching path

#### Scenario: Change that does not address an entry
- **WHEN** an OpenSpec change names `factory-agent/src/tests/connection.rs` in an artifact, an entry lists `crates/factory-agent/src/tests/connection.rs`, and no artifact of the change mentions the entry's ID
- **THEN** the lookup with `--change` prints the entry and fails

#### Scenario: Change that addresses an entry
- **WHEN** the same change names the entry's ID in its proposal, as fixed or as out of scope
- **THEN** the lookup with `--change` succeeds

#### Scenario: Lookup without a hit
- **WHEN** no entry lists a changed file or a directory above it
- **THEN** the lookup prints nothing and succeeds
