# Proposal

## Why

Iteration 1 (`product/iterations/iteration-1.md`) is split into parallel workstreams so several agents can implement it at once. They all need the same Cargo workspace, the generated contract bindings, the database schema and a few trait seams (`SandboxProvider`, `ForgeClient`, `SecretStore`) with fakes. Without a shared base, every workstream would invent its own and the merge would fail. This change builds that base and nothing more.

## What Changes

- Cargo workspace (`crates/*`, `plugins/*`), pinned toolchain, shared `[workspace.dependencies]`.
- `df-proto`: Rust bindings for `darkfactory.agent.v1` and `darkfactory.transcript.v1`, with canonical protobuf JSON (needed for JSONB storage of transcript events).
- `df-core`: domain types (UUIDv7 ids, project/task keys, task status, run state and outcome, principals), instance configuration loading, the trait seams and their in-memory fakes (`test-util` feature).
- `df-db`: PostgreSQL pool, embedded migrations for every table shared by more than one workstream, the `LISTEN/NOTIFY` notifier, the Postgres blob store, the audit-log writer and the task-management operations (create task, queue message).
- `dark-factory` binary with `migrate` (migrations, optional application-role management, grants) and empty `serve` / `gateway` subcommands that later workstreams fill.
- The workstream map (which change owns which crate, table, port and interface) in `design.md`.

## Capabilities

### New Capabilities
- `task-management`: task keys, task creation with the assignment prompt, queued messages and the task status vocabulary shared by UI and orchestration.
- `database-migrations`: `dark-factory migrate` — embedded migrations, application-role management and grants per `design/deployment.md` §4.
- `audit-log`: append-only audit entries with the acting principal and its roles.

### Modified Capabilities
None (no existing specs).

## Impact

- New: `Cargo.toml`, `rust-toolchain.toml`, `crates/df-proto`, `crates/df-core`, `crates/df-db` (incl. `migrations/`), `crates/dark-factory`.
- Every other `iter1-*` change depends on this one. `iter1-agent-runtime` needs only `df-proto` (task group 2) and can start as soon as that lands.
- Dependencies introduced: tokio, prost, tonic, pbjson, sqlx (postgres, offline mode), serde, uuid (v7), clap, tracing, thiserror, anyhow.
