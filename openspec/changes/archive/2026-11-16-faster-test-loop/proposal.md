# Proposal

## Why

Every e2e test logs in at Dex before its body runs: 34 logins per suite run, each a page load, a form submit, a code exchange and a redirect (up to `LOGIN_TIMEOUT` of 30 s on a cold cluster). `e2e-suite-isolation` left this out as a speed-up, not a robustness fix (backlog row "Reuse the login state across e2e tests"). The suite now isolates its tests, so sharing one web session between them is safe: the session only holds the user and the pending login (`df-web` `auth/mod.rs`), and token refresh is serialized per session row (`PgSessionStore::lock`).

While the e2e crate is open, a few small debts of the same crate and runner go with it.

Waiting for the compiler is the largest cost of shipping a feature with agents, and `target/` gets large fast (33 GB on one machine: 21 GB `debug/deps`, 12 GB `debug/incremental`). The workspace has no `[profile.dev]`, so every test binary carries full debug info of every dependency: 51 test binaries above 50 MB, the `df-e2e` ones above 300 MB. Writing and linking that much costs time on every build. The backlog row "Measure compile time, then less debug info in the dev profile and `cargo nextest`" was postponed twice.

## What Changes

- **One login per test binary.** The first test of a binary logs in at Dex and keeps the browser storage state (cookies) in the process. Every later test of that binary opens its browser context with that state and starts on the board without the Dex form. A context whose page still lands on the login form (session expired or removed) logs in again and replaces the shared state.
- **The login test keeps a real login.** `login_shows_the_demo_project_on_the_board` opens a fresh context and logs in through Dex, so the login flow stays covered.
- **Second session of the traffic test** (`ui.rs`, the watcher) uses the shared state too.
- **Low-hanging debts of the e2e crate and runner:**
  - TD-81 ([#123]): the queries of `crates/df-e2e/src/db.rs` become compile-time checked (`query!`, `query_scalar!`, `query_as!` with `.sqlx/` metadata). `e2e-suite-isolation` moved all e2e SQL into that one file, so this is a mechanical change in one place.
  - TD-111: `Forgejo::submit_review` takes a `ReviewEvent` enum instead of a string.
  - TD-110: `e2e-local.sh` sets Forgejo's `ALLOWED_HOST_LIST` from `$release` and `$namespace` (`--set`), the values file no longer repeats the defaults.
- **Faster local builds and a smaller `target/`** (cargo only; Bazel and CI test builds keep their own settings):
  - Measure first: clean and warm `cargo test --workspace --no-run --timings`, and the size of `target/`, before and after; the numbers go into design.md.
  - `[profile.dev]` with `debug = "line-tables-only"` for workspace crates and no debug info for dependencies. Panics and backtraces keep file and line; a debugger loses variables (override per shell with `CARGO_PROFILE_DEV_DEBUG=true`).
  - `cargo nextest` as the test runner of the local loop, pinned through `//:bazel_env` like `cargo-deny`. Doc tests still run with `cargo test --doc`. The e2e suite stays on `cargo test` (its runner sets parallelism per binary).
  - Skill `rust` and `docs/development.md` describe the new loop.
- **Stale records removed:**
  - TD-94 (fixed: `cleanup` iterates with `${pids[@]+"${pids[@]}"}`) and TD-103 (fixed: the follow-up test waits on the PR's draft flag) leave `docs/tech_debt.md`.
  - TD-93 is reworded: the second Forgejo user exists (`World::reviewer`); what remains is asserting the role `member` in the agent's tool result.
  - Roadmap backlog: the rows "Reuse the login state across e2e tests" and "Measure compile time …" (this change) and "CI job running the e2e suite" (exists: `.github/workflows/e2e.yaml`) are removed.

### Out of scope

- One login per suite run, shared across binaries through a file written by the runner: saves about 9 more logins, but needs the runner, a file outside the test and staleness handling, and plain `cargo test` of one test would depend on it. Rejected.
- TD-93 role assertions (`member`, `outside`): needs the agent's tool result checked in the e2e and a third user; not a cleanup. Stays in `docs/tech_debt.md`.
- TD-120 and TD-122 (live delivery of the traffic page and the Artifacts tab have no e2e test): new tests, not a cleanup. Stay in `docs/tech_debt.md`.
- One integration test binary per crate (`df-plugins` has 10, `df-gateway` 6): saves link time, but every Bazel `rust_test` target and `cargo xtask check-crates` change with it. Target: backlog in `product/iterations/roadmap.md`, decided with the timings of this change.
- A different linker (`mold`) or a shared `CARGO_TARGET_DIR` across worktrees: rejected with the timings of this change (design.md, Risks, "Compile-time numbers"): the arm64 build already links with `rust-lld` and linking is not shown as the bottleneck, and a shared target directory serializes parallel builds.
- Pruning stale artifacts from `target/` (a `just` recipe or `cargo-sweep`): `target/` is deleted before each change anyway. Rejected.
- Changes to Bazel's `fastbuild` settings: CI test times are not the complaint. Rejected.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `e2e-runner`: adds a requirement for a shared login per test binary, with a real login kept for the login test.

## Impact

- **Code:** `crates/df-e2e/src/browser.rs` (session start from a storage state), `fixture.rs` (shared state per process), `db.rs` (checked queries), `forgejo.rs` (`ReviewEvent`); `tests/demo.rs`, `tests/ui.rs`, `tests/webhooks.rs`; `.sqlx/` (new query metadata).
- **Runner:** `deploy/e2e/e2e-local.sh`, `deploy/e2e/forgejo-values.yaml`.
- **Build:** `Cargo.toml` (`[profile.dev]`), `.config/nextest.toml`, `tools/xtask/src/main.rs` (`TOOLS`), `BUILD.bazel` (`//:bazel_env`), `multitool.lock.json`, `justfile` (`test`).
- **Developers and agents:** the first build after the profile change recompiles everything once.
- **Docs:** `docs/demo.md`, `docs/development.md` (contributor docs), skill `rust`, `docs/tech_debt.md`, `product/iterations/roadmap.md`. No published docs change: nothing a user or operator sees or configures changes.
- **Dependencies:** no new crate; one new pinned tool (`cargo-nextest`). `playwright-rs` 0.21 has `BrowserContext::storage_state` and `Browser::new_context_with_options` with an inline storage state, so no file crosses the remote browser connection.

[#123]: https://github.com/prskr/dark-factory/issues/123
