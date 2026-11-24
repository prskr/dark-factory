# Coding standards

How code and docs in this repository are written. Implementers read it before writing code; the standards reviewer checks every diff against it. `AGENTS.md` holds the project orientation, skill `rust` the toolchain, test and lint mechanics.

## File structure

Every source file should (roughly) follow this structure:

- module doc comment (`//!`)
- `use` declarations
- constants and statics
- public types and functions
- private helper types and functions
- `#[cfg(test)] mod tests` (or `mod tests;` pointing to a `tests.rs` file for larger suites)

`impl` blocks stay next to their type. Public items come before private helpers.

Test organization:

- Keep an inline `#[cfg(test)] mod tests { … }` up to about 500 lines. Beyond that, move the body to `<module>/tests.rs` and declare `#[cfg(test)] mod tests;`.
- A test file beyond about 1,500 lines becomes a directory `tests/` with a `mod.rs` for shared fixtures and one file per topic (the module or behavior the tests exercise).
- Name tests after the behavior they check, never after a review round or finding (`test_erase_and_expiry_drop_cached_credentials`, not `test_review_r38_…`). `cargo xtask check-test-names` fails for a test name with a word `finding`, `round<n>`, `r<n>` or `td<n>`.
- Source files have at most 1,500 lines (`cargo xtask check-file-length`; exceptions: skill `rust`, "Lints").
- The 500-line threshold for inline test modules is guidance, not a check.
- Blocks nest at most 5 levels deep (`excessive-nesting-threshold` in `clippy.toml`, `clippy::excessive_nesting`). Flatten with early returns, `let … else`, iterator chains or an extracted function.

Function length:

- A function has at most 100 lines (`clippy::too_many_lines`; comments and blank lines do not count). Lint levels and thresholds live only in `[workspace.lints]` and `clippy.toml` (details: skill `rust`, "Lints").
- Split a function that gets longer. A production function that cannot be split yet gets `#[expect(clippy::too_many_lines, reason = "…")]` naming the follow-up; use `expect`, never `allow`, so the attribute fails once the function is short enough.
- Test modules with scenario tests opt out once at module level with `#![expect(clippy::too_many_lines, reason = "…")]` (or `#[expect]` on an inline `mod tests`), only where a test needs it.

Lint attributes:

- `#[allow]` is forbidden (`clippy::allow_attributes`, `clippy::allow_attributes_without_reason`). Use `#[expect(…, reason = "…")]`, which fails once the lint no longer fires. Generated code (`df-proto`, `wit_bindgen::generate!`) expects the lints it triggers at module or crate level.
- `clippy::pedantic` is enabled.

## Traits over closures

Prefer small, single-method traits over boxed closures or function pointers when a value represents a capability with a contract ("exec a command on a remote host", "persist a snapshot"). Declare the trait where it is consumed; the trait carries documentation a closure type cannot, and it is easy to fake in tests.

Use plain closures / `impl Fn` for callbacks passed inline (predicates, mappers) that are not stored on a struct and not faked in tests.

## Errors over panics

Always prefer returning a `Result` over panicking. `unwrap()`, `expect()`, indexing that can go out of bounds and `unreachable!()` are a last resort and MUST NOT happen in production code paths: any failure the caller can react to — invalid arguments, missing resources, failed operations — is returned as an error.

- Library code returns typed errors (`thiserror`); binaries and glue code may use `anyhow` and add context with `.context(...)`.
- `expect("<why this cannot fail>")` is acceptable for invariants that are guaranteed by construction (for example a compiled-in regex), never for input or I/O.
- Don't silently drop errors with `let _ =` unless ignoring is the intended behavior (for example a best-effort send on a closed channel); then say so in a short comment.
- Lints enforce this: `clippy::unwrap_used`, `panic`, `todo`, `unimplemented`, `unreachable` and `indexing_slicing` (use `.get()`, iterators or pattern matching), `let_underscore_must_use` and `let_underscore_future` (handle the error, log it, or discard it explicitly with `.ok()` and a comment), and `map_err_ignore` (bind the error as `|_elapsed|` or include it in the new error instead of `|_|`).
- Tests are exempt (next section).

## Tests

- Test code may use `unwrap()`, `expect()`, `panic!`, indexing, `dbg!` and print macros (`allow-*-in-tests` in `clippy.toml`). That covers `#[test]` functions and `#[cfg(test)]` code; helper functions of integration tests and test utilities (`df-test-postgres`) take a module-level `#![expect(…, reason = "…")]`. `unreachable!` is not exempt: use `panic!`.
- Use the standard assertion macros (`assert!`, `assert_eq!`, `assert_ne!`, `matches!`) and return `Result<()>` from tests that use `?` instead of unwrapping. Don't hand-write `if … { panic!(…) }` guards.
- Use `#[tokio::test]` for async tests and `tempfile` for files and directories. Never write to fixed paths; `std::env::temp_dir` is a disallowed method (`clippy.toml`).
- Database tests use `df_test_postgres::{migrated_pool, empty_pool, server}` (dev-dependency crate `df-test-postgres`) instead of `#[sqlx::test]`. They start a PostgreSQL container through testcontainers and therefore cannot run in a sandbox without a container socket.
- Avoid timing-based tests (`sleep` to wait for an effect). Wait on the condition, use a channel, or use `tokio::time::pause()`.
- When a bug is fixed, add a test that fails without the fix.

## Concurrency (tokio)

- Never hold a `std::sync::Mutex` guard, or a `tokio::sync::Mutex` guard that other tasks wait on, across a long `.await`.
- `tokio::select!` branches must be cancel-safe. If a branch has side effects (a message taken from a channel, a partially written buffer), finish the work outside the `select!` or use a cancel-safe API.
- Don't `abort()` a task that is handling a message you must not lose. Signal it to stop and let it finish.
- With `tokio::sync::Notify`, create the `notified()` future and call `enable()` before checking the condition, otherwise a wakeup between check and wait is lost.
- Put a timeout on every network handshake and every wait for a remote peer.
- Spawned tasks need an owner: keep the `JoinHandle` and abort it on drop or await it on shutdown.

## Dependencies

A balance of external dependencies versus internalizing everything is crucial.
The order of preference MUST BE this:
1. standard library - if feasible use something from `std`
2. already referenced dependencies - check `[workspace.dependencies]` and the lock file for something that already offers it
3. IF the amount of required code is small and does not require much maintenance it might be better to internalize it, IF the code affects security or might require frequent maintenance (e.g. consuming an external API that might change frequently) an external dependency might be better

Hand-rolling is always a tradeoff: an extra dependency on one side, extra code to write, test and maintain on the other. Don't hand-roll only to keep the dependency list short. If a crate is already in the dependency graph as a transitive dependency, using it directly costs little (a line in `[workspace.dependencies]` and one in the crate's `Cargo.toml`). Prefer that over reimplementing its functionality whenever it makes the code easier to read or cheaper to maintain.

How to add one (workspace declaration, latest release, minimal features, `cargo deny`, lock files): skill `rust`.

## CLI args parsing

- Binaries use `clap` with the derive API (`#[derive(Parser)]`, `#[derive(Subcommand)]`).
- Prefer clap's built-in features (`env`, `default_value`, `value_enum`, `requires`) over hand-written parsing.

## Logging

- Use the `tracing` macros (`info!`, `warn!`, `error!`, `debug!`) with structured fields: `tracing::warn!(error = %e, run_id = %id, "message")`. Don't interpolate values into the message.
- Never log secrets (tokens, passwords, credential helper input) and don't log whole request/response bodies.
- Library code only emits events. Subscriber setup belongs in the binary's `main`.
- `clippy::print_stdout`, `print_stderr` and `dbg_macro` enforce it. A function that prints CLI output carries `#[expect(clippy::print_stdout, reason = "CLI output")]` (or `print_stderr`); there is no crate-level exemption.

## Security

This project runs untrusted agent code. Treat everything the agent can influence (the repository contents, `.git`, files in the workspace, anything the harness sends) as untrusted input.

- Validate and normalize input before checking it against a denylist; check the normalized form that is actually used.
- Don't follow paths or symlinks that the agent can control when writing runtime-owned files.
- Secrets live in memory or in files outside the persistent volume; never write them under `/data`.

## Documentation

- User and operator documentation is the published site in `docs/content/` (plus the crate references mounted from `crates/*/docs/`, see `docs/config/_default/module.yaml`). Every change that alters what users or operators see or configure updates it in the same change; `just docs-build` checks the links. Contributor docs (`docs/*.md`, ADRs, `design/`) are not published.
- New or changed configuration options, environment variables and system behavior get a reference entry (variable, required or optional, default, effect).
- Prefer scenario-based how-to guides for common operations and diagrams (Mermaid) for state machines and interactions.
- Keep prose short and technical.
