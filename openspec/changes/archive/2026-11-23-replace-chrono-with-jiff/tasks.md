# Tasks

Bottom-up along the crate order of design D6. Group 1 runs while the workspace is still on `chrono`; from group 3 on, crates above the one being migrated do not compile until their own group. Each group ends with a WIP commit; they are squashed at the end. Database tests need PostgreSQL (skill `rust`); report the ones that could not run. No published docs change: nothing a user or operator configures changes; the RFC 3339 text changes of design D4 are not documented anywhere.

## 1. Pin the current formats (on `chrono`)

- [x] 1.1 Add tests with a fixed time that pin: the lifecycle event `timestamp` text (`df-db`, three fractional digits, `Z`); `next_since` and the `since` parsing of `df-agent-channel/src/tools/feedback.rs`; the metadata line of `tools/artifacts.rs`; the visible time text and the `<time datetime>` attribute of `df-web` views (`requests.rs`, `artifacts/view.rs`, `chat_pages/view.rs`, `chat_pages/archive.rs`, `components/transcript.rs`, `project_subscriptions.rs`, `egress/log/rows.rs`, `task_pages/view.rs`, `task_pages/resume.rs`), where a view test does not pin them yet. Verify all pass on `chrono`: `cargo nextest run -p df-db -p df-agent-channel -p df-web` with the new test names as filter.

## 2. Dependencies and spike

- [x] 2.1 Add `jiff` 1.0 (`default-features = false`, features `std`, `serde`) and `jiff-sqlx` 1.0 (feature `postgres`) to `[workspace.dependencies]`; add the `sqlx` feature `sqlx-toml` (keep `chrono` for now). Update `Cargo.lock` and `MODULE.bazel.lock` (skill `rust`). Verify `cargo build --workspace`.
- [x] 2.2 Spike, not committed: a throwaway crate with its own `sqlx.toml` (`'TIMESTAMPTZ' = "jiff_sqlx::Timestamp"`, design D2), no `sqlx` feature `chrono`, one `query!` that reads a `timestamptz` column and one that binds a `jiff_sqlx::Timestamp`, built offline with `.sqlx/` data under cargo and under Bazel (`sqlx.toml` in `compile_data`). Record the result in design.md D2 ("As built"); if Bazel does not pick up the file, switch D2 and group 4 to the fallback before continuing.

## 3. `df-proto` and `df-core`

- [x] 3.1 Write failing tests for `df_proto::time::{to_proto, from_proto}`: round trip with nanoseconds, the Unix epoch, −1.5 s (normalized to `seconds = -2`, `nanos = 500_000_000`), and `None` for nanos outside `0..1_000_000_000`. Implement them (design D3); add `jiff` to `df-proto`. Verify `cargo nextest run -p df-proto time`.
- [x] 3.2 Move `df-core` to `jiff` (`Timestamp`, `SignedDuration`; design D1), including `config::resume_backoff` and the run and task transitions. Remove `chrono` and its `cargo-machete` ignore from `df-core/Cargo.toml`. Verify `cargo nextest run -p df-core`.

## 4. `df-db`

- [x] 4.1 Add `crates/df-db/sqlx.toml` (design D2) and add it to `compile_data` of the `df-db` Bazel targets; remove the `sqlx` feature `chrono` from `[workspace.dependencies]` in the same step. Move every `df-db` module: `query_as!` targets keep `jiff::Timestamp` for NOT NULL columns and use `Option<jiff_sqlx::Timestamp>` for nullable ones, binds use `.to_sqlx()`, public signatures use `jiff::Timestamp`; `transcript/runtime.rs` uses `df_proto::time::from_proto`; `lifecycle_events.rs` uses `format!("{t:.3}")`. Verify `cargo nextest run -p df-db`, the pin test of 1.1 unchanged, `bazel --bazelrc=.bazelrc.agents build //crates/df-db/...`, and `git status .sqlx` showing no change (with the fallback: regenerate `.sqlx/` per `docs/development.md`).

## 5. Control plane and runtime crates

- [x] 5.1 Move `df-orchestrator` (`scheduler`, `retention`, `runs`, `pr/sync`, `testing`) and `df-gateway-control` (`usage_records.rs`) to `jiff`; add `jiff-sqlx` as a dev-dependency of `df-orchestrator` for its tests and as a dependency of its `test-util` feature for `testing.rs`. Verify `cargo nextest run --all-features -p df-orchestrator -p df-gateway-control`.
- [x] 5.2 Move `df-agent-channel` (`session`, `assign`, `tools/feedback.rs`, `tools/artifacts.rs`, `session/tests.rs` with dev-dependency `jiff-sqlx`) to `jiff`. Update the pinned texts of 1.1 only where design D4 says they change (`next_since` digits, artifact `updated` `+00:00` → `Z`). Verify `cargo nextest run -p df-agent-channel`.
- [x] 5.3 Move `df-web` to `jiff`, including `auth/store.rs` (dependency `jiff-sqlx`, `tower-sessions` `OffsetDateTime` via `Timestamp::from_second(unix_timestamp)`), `testkit.rs` and `task_pages/tests/resume.rs`. Update the pinned `datetime` attributes of 1.1 to `Z` (design D4); the visible texts stay. Verify `cargo nextest run -p df-web`; list the asset tests reported as ignored without `.bazel_env`.
- [x] 5.4 Move `dark-factory` (`lifecycle_events/handler.rs`, demo data, `handler/tests.rs` with dev-dependency `jiff-sqlx`) and `factory-agent` (`harness.rs`, `transcript.rs`, `runner.rs`) to `jiff` and `df_proto::time::to_proto`. Verify `cargo nextest run -p dark-factory -p factory-agent` (build the Forgejo wasm plugin first, skill `rust`).
- [x] 5.5 Move `tools/xtask/src/next_version.rs` to `jiff` and replace `chrono` in `tools/xtask/Cargo.toml`; update `tools/xtask/Cargo.lock`. Verify `cargo test` and `cargo clippy --all-targets -- -D warnings` in `tools/xtask`.

## 6. Remove `chrono`, guard it, update contributor docs

- [x] 6.1 Remove `chrono` from `[workspace.dependencies]` and every remaining manifest; update `Cargo.lock` and `MODULE.bazel.lock`. Verify `grep -rn chrono Cargo.toml crates/*/Cargo.toml plugins/*/Cargo.toml tools/*/Cargo.toml` finds nothing and `cargo tree -i chrono` lists only third-party parents.
- [x] 6.2 Add the `chrono` wrapper ban to `deny.toml` (design D5) and update the comment of the `RUSTSEC-2026-0543` ignore (only transitive dependencies remain). Verify `cargo deny check`, and that adding `chrono` to `df-core` makes it fail (revert afterwards).
- [x] 6.3 Narrow the backlog row "Replace `chrono` with `jiff` …" in `product/iterations/roadmap.md` to the remaining step: drop the ignore once `rmcp`, `openidconnect`, `oauth2` and `pbjson-types` no longer require `chrono`; reference this change. Verify by reading the row.
- [x] 6.4 Update skill `rust` (`.agents/skills/rust/SKILL.md`): in "Database access", `timestamptz` maps to `jiff_sqlx::Timestamp` through `crates/df-db/sqlx.toml` (NOT NULL `query_as!` fields stay `jiff::Timestamp`); replace the `chrono` `clock` feature example in "Adding a dependency". Verify by reading the section.

## 7. Verification

- [x] 7.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length`, `cargo nextest run --workspace --all-features`, `cargo test --workspace --doc` and `bazel --bazelrc=.bazelrc.agents test //...`. Verify all pass; list the container-backed tests that could not run.
