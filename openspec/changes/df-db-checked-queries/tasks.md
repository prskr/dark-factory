# Tasks

Attributes and docs only: no query, schema or `.sqlx/` change. The ratchet is the test (TDD here means: make clippy fail first, then add the opt-outs).

## 1. Inventory from clippy

- [ ] 1.1 In `crates/df-db/src/lib.rs`, remove `clippy::disallowed_methods` from the crate-level `#![expect]` (keep `clippy::disallowed_macros`, adjust the reason) and add `#![cfg_attr(test, expect(clippy::disallowed_methods, reason = "test fixtures"))]` (design D4). Run `cargo clippy -p df-db --all-targets --all-features -- -D warnings` and collect every `disallowed_methods` error outside test code. Verify: the list matches the table in design.md, "Context" (11 files, ~75 calls, including the `Executor::execute` calls in `migrate.rs` and `pool.rs`); where it differs, update the table and the file list in proposal.md, "Impact", before going on.

## 2. Opt-outs

- [ ] 2.1 Add `#[expect(clippy::disallowed_methods, reason = "dynamic SQL: <what>")]` to `migrate::run_migrate_locked`, `migrate::manage_application_role`, `migrate::apply_grants` and `pool::connect_pool` with the reasons of design D2. Verify: clippy reports no error in these four functions and `pool.rs` has no other error left.
- [ ] 2.2 Add `#![expect(clippy::disallowed_methods, reason = "unchecked queries, not yet converted")]` after the module doc comment of each remaining file from 1.1: `artifacts.rs`, `egress.rs`, `migrate.rs`, `model_providers.rs`, `model_usage.rs`, `notify.rs`, `requests.rs`, `task_events.rs`, `tasks.rs`, `webhooks.rs`. Verify: `cargo clippy --all-targets --all-features -- -D warnings` (whole workspace) passes; `--all-targets` builds both the lib and its tests, so an `expect` unfulfilled in either build would fail it.
- [ ] 2.3 Run `cargo test -p df-db --all-features`. Verify: all tests pass (attributes only). The tests need PostgreSQL through `df-test-postgres`: probe for a container socket first (`AGENTS.md`, "Containers and integration tests"); without one, run `cargo build -p df-db --all-targets --all-features` instead and state in the report that the container-backed tests did not run.

## 3. Prove the ratchet

- [ ] 3.1 By hand, not committed: add a module `crates/df-db/src/ratchet_probe.rs` (declared in `lib.rs`) with a function that calls `sqlx::query("SELECT 1")`. Verify: `cargo clippy -p df-db --all-targets --all-features -- -D warnings` fails on that call. (The "checked macro passes" case needs no probe: every file without an opt-out, for example `runs.rs`, already shows it.) Remove the probe. Second probe: add the file-level opt-out to `runs.rs`, which has no runtime call in production code: clippy must fail on the unfulfilled `expect`; remove it again. Record both results (command, error line) in a short "Ratchet check" note at the end of design.md.
- [ ] 3.2 Run `bazel --bazelrc=.bazelrc.agents build --config=lint //crates/df-db/...` with the probe module of 3.1 in place and without it. Verify: it fails with the probe and passes without it, so the clippy aspect reads the inner `expect` attributes like `cargo clippy` does. Add the result to the "Ratchet check" note.

## 4. Rules and docs

- [ ] 4.1 `AGENTS.md`, "Database access": add one sentence: in `df-db`, a runtime query needs an `expect` of `clippy::disallowed_methods` on its function (`"dynamic SQL: <what>"`) or file (`"unchecked queries, not yet converted"`); a new file uses checked macros only. Verify: the paragraph names both reason strings and no longer implies `df-db` is exempt from `disallowed-methods`; load skill `writing-for-agents` before editing.
- [ ] 4.2 `.agents/skills/rust/SKILL.md`, "Database access", the guard paragraph: `df-db` is exempt at crate level only from `disallowed-macros`; describe the ratchet and that converting a file means deleting its opt-out. Verify: `df-e2e` and `df-test-postgres` are still named as crate-level exemptions and `df-db` is not.
- [ ] 4.3 `clippy.toml`: update the comment above `disallowed-methods` (the `df-db` opt-out is per function or file, with the two reasons). Verify: comment only, `git diff clippy.toml` shows no change to the lists.
- [ ] 4.4 `docs/architecture.md`, rule 2: replace "crate level `df-db`" in the exceptions with the per-function or per-file opt-out in `df-db`. Verify: the rule still lists `df-e2e`, `df-test-postgres`, `df-web/src/auth/store.rs` and test code.
- [ ] 4.5 Published docs: none. The change alters nothing a user or operator sees or configures (contributor rules and lint attributes only), so `docs/content/` and the mounted crate references stay unchanged.

## 5. Final checks

- [ ] 5.1 Run the pre-push checks: `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all --check`, `cargo machete` and `openspec validate df-db-checked-queries --strict`. Verify: all pass.

## Workflow follow-up

- Archive the change after review; sync the `build-pipeline` delta.
- The backlog rows "Checked queries in `df-db`: convert the files with runtime queries" and "`sqlx-cli` in `bazel_env`" in `product/iterations/roadmap.md` stay open after the archive.
