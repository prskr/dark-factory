## ADDED Requirements

### Requirement: Lint rules have one source
Lint levels SHALL be declared only in the Cargo manifests (`[workspace.lints]` and the crates' `[lints]`) and in `clippy.toml`. The Bazel lint run SHALL apply the same lints as `cargo clippy --all-targets --all-features` to every Rust library, binary and test of the workspace, including integration tests under `tests/`. Code outside the Cargo workspace (`tools/xtask`) SHALL be linted by the pre-push hook.

#### Scenario: Lint added in the workspace manifest
- **WHEN** a lint is set to `deny` in `[workspace.lints.clippy]` and code violates it
- **THEN** both `cargo clippy --all-targets --all-features` and `bazel build --config=lint //crates/... //plugins/...` fail, without any change to `.bazelrc`

#### Scenario: Forbidden unsafe code under Bazel
- **WHEN** a crate contains an `unsafe` block
- **THEN** the Bazel build of that crate fails

#### Scenario: Integration test without Bazel target
- **WHEN** a file `tests/<name>.rs` is added to a crate without a Bazel test target
- **THEN** the repository check fails and names the file

#### Scenario: Crate without workspace lints
- **WHEN** a workspace member's `Cargo.toml` does not inherit the workspace lints, or a Rust target in its `BUILD.bazel` has no lint configuration
- **THEN** the repository check fails and names the crate or target

### Requirement: File length check
A portable check (`cargo xtask`, run by the pre-commit hook and CI) SHALL fail for a source file longer than 1,500 lines unless the file is on an allowlist in the root `Cargo.toml` with a reason. An allowlist entry for a missing file or a file within the limit SHALL fail the check.

#### Scenario: Oversized file
- **WHEN** a tracked source file grows past 1,500 lines and is not on the allowlist
- **THEN** the check fails and prints the file and its line count

#### Scenario: Stale allowlist entry
- **WHEN** an allowlisted file shrinks to 1,500 lines or fewer
- **THEN** the check fails and asks to remove the entry

### Requirement: Test name check
A portable check (`cargo xtask`, run by the pre-commit hook and CI) SHALL fail for a test whose name refers to a review round, review finding or tech-debt entry instead of the behavior it checks.

#### Scenario: Test named after a review finding
- **WHEN** a test function is named `test_review_r12_…`, `test_finding_3_…` or `test_td41_…`
- **THEN** the check fails and names the test
