# Dark Factory

## First step in a sandbox: Rust toolchain

Sandbox environments have no Rust toolchain pre-installed, but `rustup` is available. Before running anything else (`cargo`, `rustc`, `rust-analyzer`, Bazel targets that call `cargo`), install the toolchain pinned in `rust-toolchain.toml`:

```bash
# from the repository root: rustup reads rust-toolchain.toml (channel, components, targets)
rustup show active-toolchain || rustup toolchain install
rustc --version   # must print the channel from rust-toolchain.toml
cargo clippy --version && cargo fmt --version
```

- Always run these from inside the repository, otherwise `rustup` picks its default toolchain instead of the pinned one.
- `rust-toolchain.toml` is the source of truth. The same version is repeated in `MODULE.bazel` (`versions`); when you bump one, bump the other.
- If `rustup` reports a missing component or target, run `rustup component add <name>` / `rustup target add <triple>` with the names from the file instead of changing the file.
- A plain `cargo build` additionally needs `protoc` (>= 29.x) on `PATH` (or `PROTOC`). If the sandbox has none, build and test through Bazel instead (see below).

## Bazel usage

When using Bazel for any kind of operation you MUST use the `.bazelrc.agents` like this: `bazel --bazelrc=.bazelrc.agents ...` to avoid issues in the sandbox. It also makes remote execution the default for builds and tests.

## Containers and integration tests in a sandbox

- There is NO local container socket in the sandbox. Do not try to start Docker/Podman or install one; tests that need containers (for example PostgreSQL for `#[sqlx::test]`) cannot run locally there.
- The Bazel remote build executor DOES have a working container socket, so integration tests work when they run through Bazel: `bazel --bazelrc=.bazelrc.agents test //<path>/...`.
- If you need to find something out about the container environment or reproduce a problem that only shows up with containers, write the experiment as a test and run it on the remote executor via Bazel. Delete the experiment afterwards, or turn it into a proper test.

## Plain `cargo` builds

The repository MUST keep building with plain `cargo` commands (`cargo build --workspace`, `cargo test --workspace`, `cargo clippy --all-targets`) and rust-analyzer without any dependency on Bazel.

- `sqlx` query metadata is checked in under `.sqlx/` (offline mode is the default via `.cargo/config.toml`). After adding or changing a `sqlx::query!` / `query_as!` macro, regenerate it (see `docs/development.md`, "Updating `sqlx` Offline Metadata") and commit the result.
- Other generated code that Bazel needs but `cargo` cannot produce must not exist. If a generator is needed, make sure `build.rs` or checked-in output covers the plain `cargo` path as well.

## General Rules

Prioritize simplicity. If there is a simple solution that might be sufficient for a high percentage of cases, suggest that one instead of a - supposedly - "perfect" solution that is significantly more complicated.
Readability and maintainability have a higher priority than performance (in a sense of how fast is the application) and raw allocations.
Even if an algorithm has a few more allocations but can be understood in seconds, it is generally to be preferred over a slightly more performant one, if that one increases the maintenance burden significantly.

Follow the user's global rules as well: TDD (write the failing test first, then the minimum code to pass it), >70% coverage as a target, concise and fact-based documentation.

## Project Overview

Dark Factory is a Rust workspace (`Cargo.toml`, members `crates/*` and `plugins/*`). Orientation:

- `crates/dark-factory` – the main binary (control plane, `migrate` command)
- `crates/df-core`, `crates/df-db`, `crates/df-proto`, `crates/df-web` – domain logic, PostgreSQL storage (`sqlx`, migrations), generated protobuf/gRPC types, web UI
- `crates/factory-agent` – the sandbox runtime that runs a coding harness over ACP and talks to the control plane (see `crates/factory-agent/docs/`)
- `crates/fake-acp-agent` – scripted ACP agent used in tests
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

## Building and Running

### Development

```bash
# Build and run through cargo (toolchain installed as described above)
cargo build --workspace
cargo run -p dark-factory -- --help

# Same through Bazel
bazel --bazelrc=.bazelrc.agents build //...

# Format check (pinned, hermetic rustfmt) - this is what the pre-commit hook runs
bazel --bazelrc=.bazelrc.agents build --config=fmt //crates/... //plugins/...
# Local formatting when the toolchain is installed
cargo fmt --all
```

### Testing

```bash
# Unit tests of one crate
cargo test -p factory-agent

# Whole workspace; database tests need DATABASE_URL (see docs/development.md)
cargo test --workspace

# Everything, including container-backed tests, on the remote executor
bazel --bazelrc=.bazelrc.agents test //...
```

### Lint and hooks

`lefthook.yml` defines the checks: pre-commit runs rustfmt (via Bazel), `kingfisher`, `buildifier`, `shellcheck`, `buf format`, `hadolint`, `typos`, `cargo-machete` and the Helm lint/unit tests; pre-push runs `cargo clippy --all-targets --all-features -- -D warnings` and `cargo deny check`. Make sure clippy has no warnings and `cargo-machete` finds no unused dependencies before you finish. The pinned tool versions come from Bazel (`docs/development.md`, "Pinned linters").

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

## Traits over closures

Prefer small, single-method traits over boxed closures or function pointers when a value represents a capability with a contract ("exec a command on a remote host", "persist a snapshot"). Declare the trait where it is consumed; the trait carries documentation a closure type cannot, and it is easy to fake in tests.

Use plain closures / `impl Fn` for callbacks passed inline (predicates, mappers) that are not stored on a struct and not faked in tests.

## Errors over panics

Always prefer returning a `Result` over panicking. `unwrap()`, `expect()`, indexing that can go out of bounds and `unreachable!()` are a last resort and MUST NOT happen in production code paths: any failure the caller can react to — invalid arguments, missing resources, failed operations — is returned as an error.

- Library code returns typed errors (`thiserror`); binaries and glue code may use `anyhow` and add context with `.context(...)`.
- `expect("<why this cannot fail>")` is acceptable for invariants that are guaranteed by construction (for example a compiled-in regex), never for input or I/O.
- Don't silently drop errors with `let _ =` unless ignoring is the intended behavior (for example a best-effort send on a closed channel); then say so in a short comment.

## Tests

- Write the failing test first, then the implementation.
- Use the standard assertion macros (`assert!`, `assert_eq!`, `assert_ne!`, `matches!`) and return `Result<()>` from tests that use `?` instead of unwrapping. `unwrap()`/`expect()` are fine in tests. Don't hand-write `if … { panic!(…) }` guards.
- Use `#[tokio::test]` for async tests and `tempfile` for files and directories. Never write to fixed paths.
- Database tests use `#[sqlx::test]`. They need a container and therefore run on the remote executor in a sandbox.
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

Rules for adding one:
- Declare it in the root `[workspace.dependencies]` and reference it with `.workspace = true`.
- Start on the latest release. `cargo xtask outdated` (also a pre-commit hook for changed dependencies) fails when a requirement excludes the newest stable release. Holding a dependency back needs an entry with a reason under `[workspace.metadata.outdated.ignore]` in the root `Cargo.toml`.
- Enable only the features that are needed (`default-features = false` where sensible).
- Remove dependencies that are no longer used; `cargo-machete` runs in the pre-commit hook. Don't silence it with `ignored = [...]` to keep an unused dependency.
- `cargo deny check` must pass (licenses, advisories).
- Adding a dependency changes `Cargo.lock` and the Bazel lock files. Commit them together with the change.

## CLI args parsing

- Binaries use `clap` with the derive API (`#[derive(Parser)]`, `#[derive(Subcommand)]`).
- Prefer clap's built-in features (`env`, `default_value`, `value_enum`, `requires`) over hand-written parsing.
- Runtime configuration of the sandbox agent comes from environment variables and is documented in `crates/factory-agent/docs/configuration.md`. Keep that reference in sync when you add or change a setting.

## Logging

- Use the `tracing` macros (`info!`, `warn!`, `error!`, `debug!`) with structured fields: `tracing::warn!(error = %e, run_id = %id, "message")`. Don't interpolate values into the message.
- Never log secrets (tokens, passwords, credential helper input) and don't log whole request/response bodies.
- Library code only emits events. Subscriber setup belongs in the binary's `main`.

## Security

This project runs untrusted agent code. Treat everything the agent can influence (the repository contents, `.git`, files in the workspace, anything the harness sends) as untrusted input.

- Validate and normalize input before checking it against a denylist; check the normalized form that is actually used.
- Don't follow paths or symlinks that the agent can control when writing runtime-owned files.
- Secrets live in memory or in files outside the persistent volume; never write them under `/data`.

## Documentation

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
