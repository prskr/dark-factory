# Tasks

No behavior change: queries move from runtime functions to checked macros, opt-outs are attributes. The lint run is the failing test of the ratchet (1.2); the existing `df-db` tests guard each conversion, and a function without one gets its test first.

## 1. Prerequisites and inventory

- [ ] 1.1 Check the environment before anything else: a container socket (`AGENTS.md`, "Containers and integration tests"), a C compiler or Bazel for the `cargo` build (skill `rust`, "Toolchain in a sandbox"), and `sqlx-cli` (`cargo sqlx --version`; install it with `cargo install sqlx-cli --no-default-features --features postgres,rustls` if missing). Verify: `cargo sqlx prepare --check --workspace -- --all-targets` passes against a migrated database (`docs/development.md`, "Updating `sqlx` Offline Metadata"). If any piece is missing, stop and report which one (`ask_question`); do not convert queries without fresh `.sqlx/` (design, Risks).
- [ ] 1.2 In `crates/df-db/src/lib.rs`, remove `clippy::disallowed_methods` from the crate-level `#![expect]` (keep `clippy::disallowed_macros`, adjust the reason) and add `#![cfg_attr(test, expect(clippy::disallowed_methods, reason = "test fixtures"))]` (design D5). In `clippy.toml`, add `sqlx::__query_with_result` and `sqlx::__query_scalar_with_result` to `disallowed-methods` (design D4). Run `cargo clippy -p df-db --all-targets --all-features -- -D warnings`. Verify: the errors are exactly the 75 calls of the table in design.md, "Context", and none points at a `query!` / `query_as!` / `query_scalar!`. If one does, take the fallback of design D4: drop the two new entries, delete the scenario "Hidden query function" from `specs/build-pipeline/spec.md` and probe (c) from task 4.1, and record the gap in D4 and in design.md, "Risks". Where the list differs from the table, update the table and proposal.md before going on.
- [ ] 1.3 Check whether `sqlx` 0.9.0 accepts `concat!` as the SQL argument of `query_as!` (one query of `requests.rs`, compiled with `cargo check -p df-db`). Verify: the result is recorded in design.md D2, replacing "(unverified)"; the conversion of `requests.rs` and `model_providers.rs` follows it.

## 2. Dynamic SQL

- [ ] 2.1 Add `#[expect(clippy::disallowed_methods, reason = "dynamic SQL: <what>")]` to `migrate::run_migrate_locked`, `migrate::manage_application_role`, `migrate::apply_grants` and `pool::connect_pool` with the reasons of design D2. Verify: clippy reports no error in `pool.rs`, and in `migrate.rs` only the static calls 49 and 59 (in `run_migrate`). The static calls 104 and 126 are now hidden by the `expect`s on `run_migrate_locked` and `manage_application_role`; task 3.1 still converts them, a clean lint result there proves nothing.

## 3. Convert the static queries (one commit per file, design D6)

For each file: first check that every function with a runtime call has a `df-db` test that runs it and reads its result; write the missing test first and see it pass on the runtime query. Then convert the calls (design D2 rules), regenerate `.sqlx/`, and commit the file with its `.sqlx/*.json`. Verify each task: `cargo test -p df-db <module>` passes, `cargo clippy -p df-db --all-targets --all-features -- -D warnings` reports no error left in that file, and `cargo sqlx prepare --check --workspace -- --all-targets` passes.

- [ ] 3.1 `migrate.rs`: the migration lock (49, 59) and the two `pg_roles` queries (104, 126). The item-level `expect`s of 2.1 stay fulfilled by the `Executor::execute` calls.
- [ ] 3.2 `egress.rs` (452), `notify.rs` (278), `webhooks.rs` (121) and `model_usage.rs` (46, 70).
- [ ] 3.3 `artifacts.rs` (7 calls).
- [ ] 3.4 `model_providers.rs` (14 calls): `select_provider!` after the result of 1.3; 164 becomes one macro per branch; `ProviderRow` and `ApiRow` fit `query_as!` or go away.
- [ ] 3.5 `requests.rs` (12 calls): the `RequestRecord` queries after the result of 1.3; `RequestRecord` fits `query_as!` (nullable `timestamptz` as `Option<jiff_sqlx::Timestamp>`, skill `rust`).
- [ ] 3.6 `task_events.rs` (15 calls): 244 becomes one macro per branch.
- [ ] 3.7 `tasks.rs` (14 calls).
- [ ] 3.8 Any static call that cannot be converted: `#[expect(clippy::disallowed_methods, reason = "unchecked query: <why>")]` on its function and a row in design.md D2 with the reason. Verify: `grep -rn 'disallowed_methods' crates/df-db/src` lists only the crate-root `cfg_attr`, the four dynamic functions and the D2 rows; `cargo clippy --all-targets --all-features -- -D warnings` (whole workspace) passes.

## 4. Prove the ratchet

- [ ] 4.1 By hand, not committed, three probes, each followed by `cargo clippy -p df-db --all-targets --all-features -- -D warnings`: (a) a new function in `requests.rs` that calls `sqlx::query_scalar("SELECT 1")`, expected to fail on that call; (b) `#[expect(clippy::disallowed_methods, reason = "probe")]` on a function that runs only macros (for example `runs::list_active_runs`), expected to fail on the unfulfilled `expect` in both the lib and the test build; (c) a direct call of `sqlx::__query_with_result`, expected to fail. Revert each. Verify: the three results (command, error line) are recorded in a "Ratchet check" note at the end of design.md.
- [ ] 4.2 Run `bazel --bazelrc=.bazelrc.agents build --config=lint //crates/df-db/...` with probe (a) in place and without it, and `bazel --bazelrc=.bazelrc.agents build //crates/...`. Verify: the lint run fails with the probe and passes without it, the build passes with the new `.sqlx/` files; the result is added to the "Ratchet check" note.

## 5. Rules and docs

- [ ] 5.1 `AGENTS.md`, "Database access": state that `df-db` production code has no crate-level exemption from `disallowed-methods`; a runtime query needs an `expect` on its function with reason `"dynamic SQL: <what>"` or `"unchecked query: <why>"`, which reviewers check. Load skill `writing-for-agents` before editing. Verify: the paragraph names both reason forms and no longer lists `df-db` as exempt.
- [ ] 5.2 `.agents/skills/rust/SKILL.md`, "Database access": in the guard paragraph, `df-db` is exempt at crate level only from `disallowed-macros`, with the per-function rule of 5.1; in the `timestamptz` bullet, limit the sentence on runtime `sqlx::query_as` with `#[derive(FromRow)]` to test fixtures (DF-45, finding 3). Verify: no sentence of the skill presents a runtime query as an option for production code; `df-e2e` and `df-test-postgres` are still named as crate-level exemptions.
- [ ] 5.3 `clippy.toml`: update the comment above `disallowed-methods` (the `df-db` opt-out is per function; the two hidden functions are banned because they take SQL text). Verify: `git diff clippy.toml` shows only the comment and the two entries of 1.2.
- [ ] 5.4 `docs/architecture.md`, rule 2: replace "crate level `df-db`" in the exceptions with the per-function opt-out in `df-db`. Verify: the rule still lists `df-e2e`, `df-test-postgres`, `df-web/src/auth/store.rs` and test code.
- [ ] 5.5 Published docs: none. The change alters nothing a user or operator sees or configures (queries keep their behavior; contributor rules and lint attributes only), so `docs/content/` and the mounted crate references stay unchanged.

## 6. Final checks

- [ ] 6.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test -p df-db --all-features`, `SQLX_OFFLINE=true cargo build --workspace`, `cargo fmt --all --check`, `cargo machete` and `openspec validate df-db-checked-queries --strict`. Verify: all pass; if the container-backed tests could not run, the report says so.

## Workflow follow-up

- Archive the change after review; sync the `build-pipeline` delta.
- The backlog row "`sqlx-cli` in `bazel_env`" in `product/iterations/roadmap.md` stays open after the archive.
