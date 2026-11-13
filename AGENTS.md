# Dark Factory

## Product Development Stage

This is currently PRE MVP. There are no real users yet and no production installations.
In consequence this is the time for breaking changes.
If there are good reasons to break something now or to clean something up in a way that would be breaking, always suggest this in this stage, to get the best and cleanest possible MVP.

## Skills and crate rules

- Task-specific details live in skills under `.agents/skills/` (symlinked into `.claude/skills/`). Skill `rust` covers the sandbox toolchain setup, running tests (cargo vs. Bazel, filters, no container socket), adding dependencies and crates, `MODULE.bazel.lock`, and lint mechanics. Its rules are as binding as this file.
- Crates with their own rules have an `AGENTS.md` next to their `Cargo.toml` (`crates/df-web`, `crates/factory-agent`).

## Bazel usage

When using Bazel for any kind of operation you MUST use the `.bazelrc.agents` like this: `bazel --bazelrc=.bazelrc.agents ...` to avoid issues in the sandbox. Builds and tests run locally, without the remote cache (unreachable from sandboxes). On Linux every target is built as static musl (see `docs/development.md`, "Bazel: platforms, cache and images").

## Containers and integration tests in a sandbox

- Some environments have a container socket (often Podman, for example `$XDG_RUNTIME_DIR/podman/podman.sock`), sandboxes usually don't. Probe before skipping container-backed tests: `curl -sf --max-time 3 --unix-socket <socket> http://d/_ping`. If one answers, run the tests with `DOCKER_HOST=unix://<socket>`.
- Never start or install Docker/Podman. Without a socket, tests that need containers (for example the PostgreSQL started by `df-test-postgres`) fail; CI runs them. Which tests to select then: skill `rust`. State in your report which container-backed tests you could not run.

## Plain `cargo` builds

Rust code, including generated Rust code, MUST keep building with plain `cargo` commands (`cargo build --workspace`, `cargo test --workspace`, `cargo clippy --all-targets`) and rust-analyzer without any dependency on Bazel. These commands pass without non-Rust tools, apart from generators of Rust code (`protoc`).

- Compiled web assets are the exception: the scripts of `df-web` (TypeScript, compiled by `swc`) and its stylesheet (`tailwindcss`) need the pinned tools: `bazel run //:bazel_env` builds them into `.bazel_env/bin`, which must be on `PATH` (direnv does that through `.envrc`, see `docs/development.md`), or point `SWC_BIN` / `TAILWIND_BIN` at them. The third-party scripts (Mermaid, htmx, Alpine) come from the npm packages pinned in `crates/df-web/package.json`: run `pnpm install --frozen-lockfile` in `crates/df-web`. A `cargo` build without them prints a warning and embeds no compiled asset or third-party script; the tests that read them are reported as ignored. No compiled asset and no third-party script is checked in. Details: `crates/df-web/AGENTS.md`.
- `sqlx` query metadata is checked in under `.sqlx/` (offline mode is the default via `.cargo/config.toml`). After adding or changing a `sqlx::query!` / `query_as!` macro, regenerate it (see `docs/development.md`, "Updating `sqlx` Offline Metadata") and commit the result.
- Other generated code that Bazel needs but `cargo` cannot produce must not exist. If a generator is needed, make sure `build.rs` or checked-in output covers the plain `cargo` path as well.

## General Rules

Prioritize simplicity. If there is a simple solution that might be sufficient for a high percentage of cases, suggest that one instead of a - supposedly - "perfect" solution that is significantly more complicated.
Readability and maintainability have a higher priority than performance (in a sense of how fast is the application) and raw allocations.
Even if an algorithm has a few more allocations but can be understood in seconds, it is generally to be preferred over a slightly more performant one, if that one increases the maintenance burden significantly.

Follow the user's global rules as well: TDD (write the failing test first, then the minimum code to pass it), >70% coverage as a target, concise and fact-based documentation.

Change files with the harness's file edit tool, not with shell scripts (`python3` heredocs, `sed -i`, `cat >`, `perl -pi`). Edits stay reviewable and fail loudly when the target text doesn't match. Shell is fine for generated output (`cargo fmt`, lock files, code generators).

When addressing review feedback after the WIP commits of a multi-step implementation iteration were already squashed prefer to 'absorb' changes into the squashed commits.
This can be done either with `git absorb` if installed or otherwise with `git commit --fixup`.

## Project Overview

Dark Factory is a Rust workspace (`Cargo.toml`, members `crates/*` and `plugins/*`). Orientation:

- `crates/dark-factory` – the main binary (control plane, `migrate` command)
- `crates/df-core`, `crates/df-db`, `crates/df-proto`, `crates/df-web` – domain logic, PostgreSQL storage (`sqlx`, migrations), generated protobuf/gRPC types, web UI
- `crates/df-telemetry` – logging setup (`LogFormat`, `RUST_LOG`) shared by the binaries; features `clap` (`ValueEnum`), `http` (request span middleware), `otel` (OpenTelemetry; control plane and gateway only), `test-util` (log capture)
- `crates/factory-agent` – the sandbox runtime that runs a coding harness over ACP and talks to the control plane (see `crates/factory-agent/docs/`)
- `crates/fake-acp-agent` – scripted ACP agent used in tests
- `crates/df-e2e` – end-to-end suite (Playwright) and seed program for `just e2e-local`; cargo only, no Bazel targets
- `contract/` – protobuf and WIT contracts; `plugins/forgejo` – WASM plugin
- `deploy/` – Helm chart and image definitions; `design/`, `product/`, `openspec/` – design docs, specs and change proposals
- `tools/xtask` – repo tooling (`cargo xtask update-tools`)

Read `docs/development.md` for the local setup (PostgreSQL, `DATABASE_URL`, coverage, pinned linters).

### Key Technologies

- **Language:** Rust (edition 2024, toolchain from `rust-toolchain.toml`), `unsafe_code` is forbidden workspace-wide
- **Build system:** Cargo workspace and Bazel (`rules_rust`)
- **Async runtime:** `tokio`
- **RPC:** `tonic` + `prost` (control plane ⇄ agent), mTLS via `rustls`
- **Database:** PostgreSQL via `sqlx` (compile-time checked queries, offline metadata in `.sqlx/`)
- **CLI:** `clap` (derive)
- **Errors:** `thiserror` in library code, `anyhow` at application edges
- **Logging:** `tracing` (subscriber set up in the binary)
- **Deployment:** Helm chart in `deploy/helm/dark-factory`

## Building, testing and hooks

- Build: `cargo build --workspace`, or `bazel --bazelrc=.bazelrc.agents build //...`. Format: `cargo fmt --all` (the pre-commit hook checks with the pinned rustfmt through Bazel).
- Tests: `cargo test -p <crate>`, everything with `bazel --bazelrc=.bazelrc.agents test //...`; which to run when: skill `rust`.
- `lefthook.yml` runs the pre-commit checks (rustfmt, linters, Helm tests, `openspec validate`) and pre-push `cargo clippy --all-targets --all-features -- -D warnings` and `cargo deny check`. Before you finish, clippy has no warnings and `cargo-machete` finds no unused dependencies.

## Search of files and binaries

Whenever you are looking for source files or binaries, DO NOT search the WHOLE filesystem ( `/` ) if not STRICTLY necessary. This only consumes A LOT of time which is most of the time not necessary.

When searching the file system for sources, delegate it to a sub-agent that ONLY returns the relevant information instead of polluting the context unnecessarily.

# Rust Development rules

## Code discovery

Use `rust-analyzer`/LSP tooling or CodeGraph (below) before searching through the file system directly. Do not search `target/` or `bazel-*` directories.

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

## Adding a crate

Follow the checklist in skill `rust` and run `cargo xtask check-crates` when done (pre-commit hook and CI run it).

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

<!-- CODEGRAPH_START -->
## CodeGraph

In repositories indexed by CodeGraph (a `.codegraph/` directory exists at the repo root), reach for it BEFORE grep/find or reading files when you need to understand or locate code:

- **MCP tool** (when available): `codegraph_explore` answers most code questions in one call — the relevant symbols' verbatim source plus the call paths between them, including dynamic-dispatch hops grep can't follow. Name a file or symbol in the query to read its current line-numbered source. If it's listed but deferred, load it by name via tool search.
- **Shell** (always works): `codegraph explore "<symbol names or question>"` prints the same output.

If there is no `.codegraph/` directory, skip CodeGraph entirely — indexing is the user's decision.
<!-- CODEGRAPH_END -->
