## Why

`AGENTS.md` ("Database access") says every query in `df-db` is a checked `query!`/`query_as!`, but nothing enforces it: `crates/df-db/src/lib.rs` opts out of `clippy::disallowed_methods` for the whole crate, so clippy accepts any unchecked `sqlx::query(...)` there. The `question-kinds` retro (2026-11-27, finding 5) found one only in review (`close_pending` in `requests.rs`, standards finding S3). Unchanged runtime queries are never reviewed, and new ones depend on a reviewer seeing them.

## What Changes

- **Convert the static runtime queries.** About 71 of the 75 runtime calls in `df-db` production code (10 files, table in design.md, "Context") run fixed SQL. Each becomes a checked `query!`, `query_as!` or `query_scalar!`, one file per commit, with `.sqlx/` regenerated (decided by Peter on the review, DF-45). A call that picks one of two literals becomes two macros. No behavior change: each converted function keeps a passing `df-db` test.
- **Opt-outs per function only.** The crate-level opt-out in `lib.rs` keeps `clippy::disallowed_macros` (the checked macros need it) and drops `clippy::disallowed_methods`. Test code keeps its opt-out through `#![cfg_attr(test, expect(clippy::disallowed_methods, reason = "test fixtures"))]` at the crate root. In production code a runtime query needs `#[expect(clippy::disallowed_methods, reason = "...")]` on its function; there are no file-level opt-outs (design D1).
- **What keeps an opt-out.** The four functions with dynamic SQL (DDL and `SET search_path` with validated identifiers, through `sqlx::Executor::execute` with `AssertSqlSafe`): `run_migrate_locked`, `manage_application_role` and `apply_grants` in `migrate.rs`, `connect_pool` in `pool.rs`, each with reason `"dynamic SQL: <what>"`. A static query that turns out not to be convertible gets reason `"unchecked query: <why it cannot be a macro>"` and an entry in design.md (design D2).
- **Two hidden `sqlx` functions banned.** `sqlx::__query_with_result` and `sqlx::__query_scalar_with_result` take SQL text and are not in `disallowed-methods`; they are added, once the lint run shows that the checked macros' expansions do not trigger them (design D4).
- **Effect.** Every new function in `df-db`, in any file, can only use checked macros. `expect` fails clippy once nothing triggers it, so an opt-out cannot outlive its runtime query.
- **Rules written down.** `AGENTS.md` ("Database access"), skill `rust` ("Database access"), the comment above `disallowed-methods` in `clippy.toml` and rule 2 in `docs/architecture.md` state the rule and the two reason forms.

## Out of scope

- `sqlx-cli` in `bazel_env`, so any sandbox can run `cargo sqlx prepare` (retro finding 6): backlog row "`sqlx-cli` in `bazel_env`" in `product/iterations/roadmap.md`. This change needs `.sqlx/` regenerated; task 1.1 checks that the implementer's environment can do it before any conversion starts.
- A baseline tool outside clippy (a grep-based `xtask` check with a list of allowed call sites): rejected (design D1).
- A check of the reason text of an opt-out: rejected. Clippy accepts any reason; the two forms are a review rule in `AGENTS.md`, not a spec rule (design D3).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `build-pipeline`: new requirement that unchecked runtime SQL in `df-db` production code fails the lint run unless its function carries an expectation of the lint.

## Impact

- **`df-db`:** `src/lib.rs` (crate-level attributes); queries converted to macros in `artifacts.rs`, `egress.rs`, `migrate.rs`, `model_providers.rs`, `model_usage.rs`, `notify.rs`, `requests.rs`, `task_events.rs`, `tasks.rs`, `webhooks.rs`; function attributes in `migrate.rs` and `pool.rs`. Row types used only by runtime queries (`#[derive(FromRow)]` structs such as `RequestRecord`, `ProviderRow`) change to fit `query_as!` or go away. `.sqlx/` gains one file per converted query. No schema change, no change to the public `df-db` API.
- **Lint config and rules:** `clippy.toml` (two entries, comment), `AGENTS.md`, `.agents/skills/rust/SKILL.md`, `docs/architecture.md`.
- **Bazel:** the lint config (`.bazelrc`, `build:lint`) already passes `//:clippy.toml` to the clippy aspect, and `//:sqlx_data` already globs `.sqlx/*.json`. No change expected, checked by task 4.2.
- **Roadmap:** one backlog row (see Out of scope).
- **Domain model:** unchanged. No glossary term, no ADR (the decision is cheap to reverse).
