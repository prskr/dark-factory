# Dark Factory

## Product Development Stage

This is currently PRE MVP. There are no real users yet and no production installations.
In consequence this is the time for breaking changes.
If there are good reasons to break something now or to clean something up in a way that would be breaking, always suggest this in this stage, to get the best and cleanest possible MVP.

## Skills and crate rules

- Task-specific details live in skills under `.agents/skills/` (symlinked into `.claude/skills/`). Skill `rust` covers the sandbox toolchain setup, running tests (cargo vs. Bazel, filters, no container socket), adding dependencies and crates, `MODULE.bazel.lock`, and lint mechanics. Skill `e2e` covers the end-to-end suite (`crates/df-e2e`, `deploy/e2e`, `just e2e-local`): running it, test isolation and waits. Their rules are as binding as this file.
- Crates with their own rules have an `AGENTS.md` next to their `Cargo.toml` (`crates/df-web`, `crates/factory-agent`).

## Coding standards

`CODING_STANDARDS.md` holds the rules for writing code and docs: Rust file structure and function length, errors over panics, tests, tokio concurrency, dependencies, logging, security for agent-controlled input, and the published docs. Read it before you write code or docs; the standards reviewer checks every change against it.

## Bazel usage

When using Bazel for any kind of operation you MUST use the `.bazelrc.agents` like this: `bazel --bazelrc=.bazelrc.agents ...` to avoid issues in the sandbox. Builds and tests run locally, without the remote cache (unreachable from sandboxes). On Linux every target is built as static musl (see `docs/development.md`, "Bazel: platforms, cache and images").

## Containers and integration tests in a sandbox

- Some environments have a container socket (often Podman, for example `$XDG_RUNTIME_DIR/podman/podman.sock`), sandboxes usually don't. Probe before skipping container-backed tests: `curl -sf --max-time 3 --unix-socket <socket> http://d/_ping`. If one answers, run the tests with `DOCKER_HOST=unix://<socket>`.
- Never start or install Docker/Podman. Without a socket, tests that need containers (for example the PostgreSQL started by `df-test-postgres`) fail; CI runs them. Which tests to select then: skill `rust`. State in your report which container-backed tests you could not run.

## Plain `cargo` builds

Rust code, including generated Rust code, MUST keep building with plain `cargo` commands (`cargo build --workspace`, `cargo test --workspace`, `cargo clippy --all-targets`) and rust-analyzer without any dependency on Bazel. These commands pass without non-Rust tools, apart from generators of Rust code (`protoc`).

- Compiled web assets are the exception: the scripts of `df-web` (TypeScript, compiled by `swc`) and its stylesheet (`tailwindcss`) need the pinned tools: `bazel --bazelrc=.bazelrc.agents run //:bazel_env` builds them into `.bazel_env/bin`, which must be on `PATH` (direnv does that through `.envrc`, see `docs/development.md`), or point `SWC_BIN` / `TAILWIND_BIN` at them. The third-party scripts (Mermaid, htmx, Alpine) come from the npm packages pinned in `crates/df-web/package.json`: run `pnpm install --frozen-lockfile` in `crates/df-web` (`bazel_env` puts `pnpm` into `.bazel_env/bin` too). A `cargo` build without them prints a warning and embeds no compiled asset or third-party script; the tests that read them are reported as ignored. No compiled asset and no third-party script is checked in. Details: `crates/df-web/AGENTS.md`.
- `sqlx` query metadata is checked in under `.sqlx/` (offline mode is the default via `.cargo/config.toml`). After adding or changing a `sqlx::query!` / `query_as!` macro, regenerate it (see `docs/development.md`, "Updating `sqlx` Offline Metadata") and commit the result.
- Other generated code that Bazel needs but `cargo` cannot produce must not exist. If a generator is needed, make sure `build.rs` or checked-in output covers the plain `cargo` path as well.

## Database access

Production code reads and writes the database only through `df-db`: every query lives in the `df-db` module of the table it touches, as a checked `query!`/`query_as!`. Other crates call those functions and may pass a `PgPool` or a `Transaction` around, but never use `sqlx::query*`, `sqlx::raw_sql`, `sqlx::QueryBuilder` or `sqlx::Executor` methods with SQL text. Clippy enforces it (`disallowed-methods`, `disallowed-macros` and `disallowed-types` in `clippy.toml`). Exceptions: `df-web/src/auth/store.rs` (`web_sessions`, a session-store adapter that picks its SQL at runtime), `df-e2e` and `df-test-postgres`, and test code: `#[cfg_attr(test, expect(...))]` at crate roots, `#![expect]` in integration test files and in `test-util` fixture modules (for example `df-orchestrator/src/testing.rs`). See skill `rust`, "Database access".

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
- `crates/df-telemetry` – logging setup (`LogFormat`, `RUST_LOG`) shared by the binaries; features `clap` (`ValueEnum`), `http` (request span middleware), `otel` (OpenTelemetry; control plane and gateway only), `test-util` (log capture, metric probe)
- `crates/df-gateway`, `crates/df-gateway-control` – the egress gateway (data plane, no database; feature `test-util` exposes `link::PlainConnector`) and the control plane's side of its links (policy service, records, log relay; uses `df-db`)
- `crates/df-sandbox-contract` – what control plane and sandbox runtime share about the sandbox: path constants, path globs, `LandlockMode`
- `crates/df-tls` – mTLS acceptor with certificate reload, peer identity from client certificates; feature `test-util` (`TestPki`)
- `crates/df-orchestrator` – control-plane orchestration: run scheduler, run end and sandbox teardown, run context loading, pull request lifecycle, issue creation for approved `create-issue` requests (`issue.create` outbox job), forge webhook application, task volume retention (`OrchestratorContext`; feature `test-util` exposes `testing::Env`); never depends on `df-agent-channel`
- `crates/df-agent-channel` – the agent port: mTLS gRPC server, agent sessions, the `Assign` message, factory tools, run metrics; builds on `df-orchestrator` (`ChannelContext` wraps `OrchestratorContext`; feature `test-util` exposes the test server and runtime client)
- `crates/factory-agent` – the sandbox runtime that runs a coding harness over ACP and talks to the control plane (see `crates/factory-agent/docs/`)
- `crates/fake-acp-agent` – scripted ACP agent used in tests
- `crates/df-e2e` – end-to-end suite (Playwright) and seed program for `just e2e-local`; cargo only, no Bazel targets
- `contract/` – protobuf and WIT contracts; `plugins/forgejo` – WASM plugin
- `deploy/` – Helm chart and image definitions; `design/`, `product/`, `openspec/` – design docs, specs and change proposals
- `tools/xtask` – repo tooling (`cargo xtask update-tools`)

Read `docs/development.md` for the local setup (PostgreSQL, `DATABASE_URL`, coverage, pinned linters).

Crate ownership and allowed dependencies: `docs/architecture.md`. Place new code in the crate that owns it there; a new workspace dependency or a new crate updates that file in the same change.

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
- Tests: `cargo nextest run -p <crate>` (`cargo test -p <crate>` without `.bazel_env`), everything with `bazel --bazelrc=.bazelrc.agents test //...`; which to run when: skill `rust`.
- `lefthook.yml` runs the pre-commit checks (rustfmt, linters, Helm tests, `openspec validate`) and pre-push `cargo clippy --all-targets --all-features -- -D warnings` and `cargo deny check`. Before you finish, clippy has no warnings and `cargo-machete` finds no unused dependencies.

## Search of files and binaries

Whenever you are looking for source files or binaries, DO NOT search the WHOLE filesystem ( `/` ) if not STRICTLY necessary. This only consumes A LOT of time which is most of the time not necessary.

When searching the file system for sources, delegate it to a sub-agent that ONLY returns the relevant information instead of polluting the context unnecessarily.

## Code discovery

Use `rust-analyzer`/LSP tooling or CodeGraph (below) before searching through the file system directly. Do not search `target/` or `bazel-*` directories.

<!-- CODEGRAPH_START -->
## CodeGraph

In repositories indexed by CodeGraph (a `.codegraph/` directory exists at the repo root), reach for it BEFORE grep/find or reading files when you need to understand or locate code:

- **MCP tool** (when available): `codegraph_explore` answers most code questions in one call — the relevant symbols' verbatim source plus the call paths between them, including dynamic-dispatch hops grep can't follow. Name a file or symbol in the query to read its current line-numbered source. If it's listed but deferred, load it by name via tool search.
- **Shell** (always works): `codegraph explore "<symbol names or question>"` prints the same output.

If there is no `.codegraph/` directory, skip CodeGraph entirely — indexing is the user's decision.
<!-- CODEGRAPH_END -->
