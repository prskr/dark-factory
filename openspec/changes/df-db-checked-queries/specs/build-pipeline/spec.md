## ADDED Requirements

### Requirement: Checked queries in the storage crate
Production code of `df-db` SHALL run SQL only through compile-time checked query macros. A runtime query (a function or `Executor` method in `disallowed-methods` of `clippy.toml`) SHALL fail `cargo clippy --all-targets --all-features -- -D warnings` and the Bazel lint run unless the function that holds it carries an `expect` of `clippy::disallowed_methods`. Test code SHALL stay exempt.

#### Scenario: Runtime query in a new function of an existing file
- **WHEN** a new production function in `df-db/src/requests.rs` calls `sqlx::query_scalar("...")` without an `expect` on that function
- **THEN** both `cargo clippy --all-targets --all-features -- -D warnings` and `bazel build --config=lint //crates/df-db/...` fail and name the call

#### Scenario: Checked macro
- **WHEN** a production function of `df-db` runs its SQL through `sqlx::query!`, `query_as!` or `query_scalar!`
- **THEN** the lint run passes without any attribute on that function

#### Scenario: Hidden query function
- **WHEN** production code of `df-db` calls `sqlx::__query_with_result` or `sqlx::__query_scalar_with_result` directly
- **THEN** the lint run fails as for any other runtime query

#### Scenario: Stale opt-out
- **WHEN** a function carries an `expect` of `clippy::disallowed_methods` but runs no runtime query
- **THEN** the lint run fails on the unfulfilled `expect` until it is deleted

#### Scenario: Dynamic SQL
- **WHEN** a `df-db` function runs SQL text built at runtime (DDL with validated identifiers) and carries an `expect` of `clippy::disallowed_methods`
- **THEN** the lint run passes

#### Scenario: Test fixtures
- **WHEN** test code of `df-db` (`#[cfg(test)]` modules) runs a runtime query
- **THEN** the lint run passes
