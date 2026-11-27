## Why

`AGENTS.md` ("Database access") says every query in `df-db` is a checked `query!`/`query_as!`, but nothing enforces it: `crates/df-db/src/lib.rs` opts out of `clippy::disallowed_methods` for the whole crate, so clippy accepts any unchecked `sqlx::query(...)` there. The `question-kinds` retro (2026-11-27, finding 5) found one only in review (`close_pending` in `requests.rs`, standards finding S3). Unchanged runtime queries are never reviewed, and new ones depend on a reviewer seeing them.

## What Changes

- **Ratchet in `df-db`.** The crate-level opt-out keeps `clippy::disallowed_macros` (the checked macros need it) and drops `clippy::disallowed_methods`. Test code keeps its opt-out through `#![cfg_attr(test, expect(clippy::disallowed_methods, reason = "test fixtures"))]` at the crate root, the pattern `AGENTS.md` names for test code.
- **Named opt-outs for today's runtime queries.** Each production file that still runs an unchecked query gets a file-level `#![expect(clippy::disallowed_methods, reason = "unchecked queries, not yet converted")]`. Recount on `main` at `6a75601` (grep; task 1.1 confirms with clippy): about 75 calls in 11 files (table in design.md, "Context").
- **Dynamic SQL gets an item-level opt-out.** Only four functions build SQL at runtime (DDL and `SET search_path` with validated identifiers, run through `sqlx::Executor::execute` with `AssertSqlSafe`): `run_migrate_locked`, `manage_application_role` and `apply_grants` in `migrate.rs`, `connect_pool` in `pool.rs`. Each gets `#[expect(clippy::disallowed_methods, reason = "dynamic SQL: <what>")]` on the function (design D2).
- **Effect.** A new `df-db` file, or a new function in a file without an opt-out, can only use checked macros. Converting a file means turning its calls into macros and deleting its `expect`; `expect` fails clippy once nothing triggers it, so a stale opt-out cannot stay.
- **Rules written down.** `AGENTS.md` ("Database access"), skill `rust` ("Database access", the guard paragraph), the comment above `disallowed-methods` in `clippy.toml` and rule 2 in `docs/architecture.md` state the ratchet.
- No behavior change: attributes and docs only.

## Out of scope

- Converting the "not yet converted" files to checked macros, one file per commit with `.sqlx/` regenerated: backlog row "Checked queries in `df-db`: convert the files with runtime queries" in `product/iterations/roadmap.md`.
- `sqlx-cli` in `bazel_env`, so a sandbox can run `cargo sqlx prepare` (retro finding 6): backlog row "`sqlx-cli` in `bazel_env`" in `product/iterations/roadmap.md`. The conversion above needs it in a sandbox.
- A baseline tool outside clippy (a grep-based `xtask` check with a list of allowed call sites): rejected (design D1).
- Catching new runtime calls inside the 11 files that keep a file-level opt-out: rejected for this change; reviewers keep watching them and the conversion follow-up removes the gap file by file (design, Risks).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `build-pipeline`: new requirement that unchecked runtime SQL in `df-db` production code fails the lint run unless the function or file carries an expectation that names its reason.

## Impact

- **`df-db`:** `src/lib.rs` (crate-level attributes); inner attributes at the top of `artifacts.rs`, `egress.rs`, `migrate.rs`, `model_providers.rs`, `model_usage.rs`, `notify.rs`, `requests.rs`, `task_events.rs`, `tasks.rs`, `webhooks.rs`; function attributes in `migrate.rs` and `pool.rs`. No query, schema or `.sqlx/` change.
- **Lint config and rules:** `clippy.toml` (comment only), `AGENTS.md`, `.agents/skills/rust/SKILL.md`, `docs/architecture.md`.
- **Bazel:** the lint config (`.bazelrc`, `build:lint`) already passes `//:clippy.toml` to the clippy aspect; it reads the same attributes. No change expected, checked by task 3.2.
- **Roadmap:** two backlog rows (see Out of scope).
- **Domain model:** unchanged. No glossary term, no ADR (the decision is cheap to reverse: delete attributes).
