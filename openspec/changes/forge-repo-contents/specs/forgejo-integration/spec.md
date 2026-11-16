## ADDED Requirements

### Requirement: Repository contents
The plugin SHALL export `contents`. It SHALL first resolve the revision to a commit, then read the directory or file at that commit through Forgejo's API, so both the content and the named commit come from the same snapshot. Git LFS pointer files SHALL be returned as stored, not resolved.

#### Scenario: List at a branch
- **WHEN** `list` is called for path `.dark-factory` at branch `main`
- **THEN** the plugin returns the directory's entries and the commit `main` pointed to

#### Scenario: Tag revision
- **WHEN** `get-file` is called for `README.md` at tag `v1.0`
- **THEN** the plugin returns the file's content at the tag's commit

#### Scenario: Large file not decoded
- **WHEN** `get-file` is called for a file of 2 MiB
- **THEN** the plugin returns `too-large` with size 2,097,152
