## ADDED Requirements

### Requirement: Checked queries in the storage crate
Production code of `df-db` SHALL run SQL only through compile-time checked query macros. A runtime query (the functions and `Executor` methods in `disallowed-methods` of `clippy.toml`) SHALL fail `cargo clippy --all-targets --all-features -- -D warnings` and the Bazel lint run, unless its function or file carries an `expect` of `clippy::disallowed_methods` whose reason names dynamic SQL or a file not yet converted. Test code SHALL stay exempt.

#### Scenario: Runtime query in a new file
- **WHEN** a new production module of `df-db` without an opt-out calls `sqlx::query("...")`
- **THEN** both `cargo clippy --all-targets --all-features -- -D warnings` and `bazel build --config=lint //crates/df-db/...` fail and name the call

#### Scenario: Checked macro in a new file
- **WHEN** a new production module of `df-db` runs its SQL through `sqlx::query!`, `query_as!` or `query_scalar!`
- **THEN** the lint run passes without any attribute in that module

#### Scenario: File fully converted
- **WHEN** the last runtime query of a file with a file-level opt-out becomes a checked macro and the opt-out stays
- **THEN** the lint run fails on the unfulfilled `expect` until the opt-out is deleted

#### Scenario: Dynamic SQL
- **WHEN** a `df-db` function runs SQL text built at runtime (DDL with validated identifiers) and carries an item-level `expect` whose reason starts with `dynamic SQL:`
- **THEN** the lint run passes, and a runtime query added to another function of a file without a file-level opt-out still fails

#### Scenario: Test fixtures
- **WHEN** test code of `df-db` (`#[cfg(test)]` modules) runs a runtime query
- **THEN** the lint run passes
