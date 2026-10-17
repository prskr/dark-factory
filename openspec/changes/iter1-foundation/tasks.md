# Tasks

## 1. Workspace

- [ ] 1.1 Create root `Cargo.toml` (members `crates/*`, `plugins/*`, resolver 3, `[workspace.dependencies]` for tokio, prost, tonic, pbjson, sqlx, serde, uuid v7, clap, tracing, thiserror, anyhow, futures, async-trait), `rust-toolchain.toml` (stable, targets `x86_64-unknown-linux-musl`, `wasm32-wasip2`) and `.cargo/config.toml`; verify `cargo metadata` succeeds
- [ ] 1.2 Add `cargo deny` config (licenses, advisories, ban `aws-lc-rs` in favour of `ring`); verify `cargo deny check` passes on the empty workspace

## 2. Contract bindings (`df-proto`)

- [ ] 2.1 Create `crates/df-proto` with `build.rs` (tonic-build + pbjson-build over `contract/proto`, protoc from `PROTOC`); verify `cargo build -p df-proto` produces `agent::v1` and `transcript::v1` modules including the `AgentChannel` client and server
- [ ] 2.2 Write a JSON round-trip test: an `Event` with each `kind` serialises to canonical protobuf JSON (camelCase, enum names, int64 as string) and back unchanged; verify the test passes

## 3. Domain types and seams (`df-core`)

- [ ] 3.1 Write tests first, then implement `ProjectKey` (regex `^[A-Z][A-Z0-9]{1,9}$`), `TaskKey` (`<PROJECT>-<n>`, parse/display) and derived names (task branch, WIP branch, volume name); verify the `DF-1` naming scenario passes
- [ ] 3.2 Implement UUIDv7 id newtypes (`ProjectId`, `TaskId`, `RunId`, …), `TaskStatus`, `RunState`, `RunOutcome` (incl. control-plane outcomes) and `Principal` with string round-trip tests matching the DB check constraints
- [ ] 3.3 Implement `InstanceConfig` YAML loading (file path secrets, per-section structs) with a test fixture; verify a missing required field yields an error naming the field
- [ ] 3.4 Define `SandboxProvider`, `ForgeClient`, `SecretStore` and their value types/errors per design D3; verify `cargo doc -p df-core` builds
- [ ] 3.5 Implement `testing::{FakeSandboxProvider, FakeForgeClient, FakeSecretStore}` (feature `test-util`) recording calls and allowing scripted results/events; verify with unit tests for each fake

## 4. Database (`df-db`)

- [ ] 4.1 Write migrations for all tables in design D4 (check constraints for keys and statuses, partial unique index for one active run per task, lz4 compression on `transcript_events.payload`); verify `sqlx migrate run` against an empty database
- [ ] 4.2 Implement pool setup (schema search path, password from file) and `notify` publish/subscribe (D5); verify with a `#[sqlx::test]` that a notification published on one connection reaches a subscriber
- [ ] 4.3 Implement task-management operations test-first: create project-scoped task with gap-free numbering (row lock on `projects.next_task_number`), queue the `task.assigned` prompt in the same transaction, queue message (status → `queued`), mark prompt delivered; verify the scenarios in `specs/task-management/spec.md` incl. concurrent creation
- [ ] 4.4 Implement the audit writer (actor, roles, action, resource, details) and the Postgres blob store (`put`/`get` by SHA-256, idempotent); verify with `#[sqlx::test]`s, incl. that details never accept a `SecretString`
- [ ] 4.5 Check in sqlx offline data (`cargo sqlx prepare --workspace`); verify `SQLX_OFFLINE=true cargo build` works without a database

## 5. Binary and migrate command

- [ ] 5.1 Create `crates/dark-factory` with clap subcommands `serve`, `migrate`, `gateway` (the latter two placeholders log "not implemented" and exit 1), tracing setup and `--config`; verify `dark-factory --help` lists all three
- [ ] 5.2 Implement `migrate` test-first: advisory-locked migrations, optional app-role create/rotate from a password file, grants and the audit-log revoke per `design/deployment.md` §4.2; verify the scenarios in `specs/database-migrations/spec.md` with an integration test using a superuser-created owner role
- [ ] 5.3 Document local development (Postgres container, `DATABASE_URL`, `cargo sqlx prepare`, config file) in `docs/development.md` and verify the documented commands run as written

## 6. Handover

- [ ] 6.1 Verify `cargo test --workspace` and `cargo llvm-cov --workspace` report ≥ 70 % line coverage for `df-core` and `df-db`
