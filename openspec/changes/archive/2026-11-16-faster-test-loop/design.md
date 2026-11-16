# Design

## Context

`fixture::World::start` (`crates/df-e2e/src/fixture.rs`) calls `Session::start` (new Playwright driver, connection to the remote browser, new context, new page) and `Session::login` (Dex form, wait for the redirect to the UI, read the cookie header for the teardown's cancel fallback). It does this for each of the 34 fixture tests; the traffic test in `ui.rs` builds a second `Session` and logs in again.

Every `#[tokio::test]` has its own runtime, and the tests of one binary run as threads of one process (`--test-threads=2`). Test binaries are separate processes.

The web session (`df-web`, `PgSessionStore`) holds the user and the pending login; token refresh locks the session row (`FOR UPDATE`), so parallel requests with the same cookie are safe. Sessions survive control plane restarts.

`playwright-rs` 0.21: `BrowserContext::storage_state(None)` returns the cookies and local storage as a value over the connection; `Browser::new_context_with_options` takes it inline (`BrowserContextOptions::storage_state`). No file is written on the browser server, unlike the trace (`e2e-suite-isolation`, Risks).

## Goals / Non-Goals

**Goals:**
- The `Shared login per test binary` requirement of the `e2e-runner` delta.
- The cleanups listed in the proposal (TD-81, TD-110, TD-111, stale records).
- Shorter clean and warm cargo builds of the test binaries, and a smaller `target/` per build, both measured.

**Non-Goals:**
- Sharing the browser, the driver or the page between tests. Each test keeps its own context and page, so console logs, screenshots and open event streams stay per test. Rejected.
- One login per suite run across binaries: proposal, out of scope (rejected).
- Pruning stale artifacts from `target/` (a recipe or `cargo-sweep`): developers delete `target/` before each change, so a build never lives long enough for pruning to matter; the smaller binaries of D7 cover the size of one build. Rejected.
- Bazel builds and CI test runs: unchanged (`compilation_mode`, remote cache). Rejected for this change; the complaint is the local and agent loop.

## Decisions

### D1: The stored state lives in a process-wide static in `browser.rs`

`static SHARED_STATE: tokio::sync::Mutex<Option<StorageState>>`. `Session::start_logged_in(env)`:

1. Takes a clone of the stored state (lock held only for the clone).
2. With a state: opens the context with it, goes to `env.base_url`. If the page stays on the UI (URL starts with `base_url`), the session is ready.
3. Without a state, or when the page landed on Dex: locks the mutex, re-checks whether another test stored a newer state meanwhile (then retries step 2 once with it), else runs the Dex login on this page and stores `context.storage_state()`. The lock is held across the login, so two tests starting together log in once.
4. Reads the cookie header for the teardown fallback, as `login` does today.

`World::start` and the watcher in `ui.rs` call `start_logged_in`. `Session::start` and `Session::login` stay public for the login test.

**Why a `tokio::sync::Mutex` held across the login:** the waiting test needs exactly the result of that login; waiting is the point. The project rule against long `.await` under a mutex targets locks that guard unrelated work. tokio's sync types do not depend on a runtime, so tests on different runtimes share it.

**Alternatives considered:**
- `tokio::sync::OnceCell`: cannot be replaced after an expired session.
- A file written by the first test (`target/e2e-state.json`): a stale file from an earlier cluster would be used first; the in-memory state dies with the process.
- `std::sync::OnceLock` without coordination: two parallel tests would both log in on a fresh binary; harmless, but the mutex costs no more code.

### D2: The login test logs in on a fresh session in its body

`login_shows_the_demo_project_on_the_board` builds `Session::start` + `Session::login` itself and checks the board on that page. The fixture's own session is unused by it. No new fixture variant.

### D3: Detecting "not logged in"

After `goto(base_url)` the UI redirects an unauthenticated request to Dex. The check is the URL prefix (`base_url`), the same check `login` uses for its landing, with a short timeout (`UI_TIMEOUT`). No extra endpoint.

### D4: Checked queries in `df-e2e` (TD-81)

The 22 runtime queries of `db.rs` become `query!`, `query_scalar!` and `query_as!`. Metadata goes into the workspace `.sqlx/` through `cargo sqlx prepare --workspace -- --all-targets` (docs/development.md §4); `df-e2e` is cargo-only, so no Bazel `compile_data` is needed. Columns read as `TaskStatus` or `RunOutcome` keep their parse through the existing `FromStr`/serde path; a query whose result type sqlx infers as nullable gets the `"col!"` override only where the schema guarantees `NOT NULL` through a join.

**Alternative considered:** reading through `df-db` functions. Most e2e reads (tool calls, counts per run, audit entries) have no `df-db` function; adding them would grow `df-db` for tests only. Rejected.

### D5: Small runner and helper fixes

- **TD-111:** `pub enum ReviewEvent { Approve, RequestChanges, Comment }` with `as_str()` mapping to Forgejo's `APPROVED`/`REQUEST_CHANGES`/`COMMENT`; `submit_review(number, ReviewEvent, body)`.
- **TD-110:** `helm upgrade --install forgejo … --set gitea.config.webhook.ALLOWED_HOST_LIST=$release-control-plane.$namespace.svc` in `e2e-local.sh` (key path as in `forgejo-values.yaml`); the line leaves the values file.

### D6: Measure before and after, with fixed commands

Baseline and result use the same commands on the same machine, recorded in a table in this file (Risks, "Compile-time numbers"):

- **Clean:** `cargo clean && cargo test --workspace --no-run --timings` (wall time, `target/cargo-timings/cargo-timing.html` kept as evidence, not committed).
- **Warm, low crate:** `touch crates/df-core/src/lib.rs && cargo test --workspace --no-run` (what a change in a widely used crate costs).
- **Warm, leaf crate:** `touch crates/df-web/src/lib.rs && cargo test -p df-web --no-run`.
- **Size:** `du -sh target/debug target/debug/deps target/debug/incremental` after the clean build.

The timings also show whether linking or proc macros dominate; that decides the backlog rows of the proposal's out of scope (one test binary per crate, `mold`).

### D7: `[profile.dev]` with less debug info

```toml
[profile.dev]
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false
```

`profile.test` inherits `dev`, so test binaries shrink too. Line tables keep file and line in panics and `RUST_BACKTRACE` output, which is what agents and CI logs use; dependencies need no debug info because nobody steps into them.

**Alternatives considered:**
- `debug = false` everywhere: backtraces lose line numbers. Rejected.
- `split-debuginfo = "unpacked"`: smaller binaries but debug files next to them; with line tables only the gain is small. Revisit only if D6 shows linking still dominates.
- `[profile.dev.build-override] opt-level = 3` (faster `sqlx` macros and build scripts): only if D6 shows proc macros as a large share; then a one-line addition in this change.

### D8: `cargo nextest` for the local test loop

- **Pinned** like `cargo-deny` and `cargo-machete`: an entry in `TOOLS` (`tools/xtask/src/main.rs`) and in `//:bazel_env`, so it lands in `.bazel_env/bin` (docs/development.md, "Tools").
- **`.config/nextest.toml`:** the default profile with `slow-timeout` so a hanging test is named, and `fail-fast = false` for the before-commit run. No retries (the suite must not hide flakiness).
- **`df_test_postgres`** already shares one reusable container across processes (`crates/df-test-postgres/src/lib.rs`, "Container lifecycle"), so one process per test works without a change; each process reconnects to the running container.
- **Doc tests:** nextest does not run them; `just test` runs `cargo nextest run --workspace` and then `cargo test --workspace --doc`.
- **Not moved:** the e2e suite (`e2e-local.sh` runs `cargo test` per binary with `--test-threads`), Bazel tests in CI, `cargo llvm-cov` (it can use nextest later with `cargo llvm-cov nextest`; not needed now).

**Why nextest:** binaries are run in parallel instead of one after another (the workspace has 37 integration test binaries), a slow or hanging test is reported by name, and filters work across binaries (`-E 'test(/pattern/)'`).

## Risks / Trade-offs

- **[Compile-time numbers]** Filled in by tasks 1.1 and 1.4: a table of clean, warm (low crate), warm (leaf crate) and `target/` size, before and after.

  Machine: Linux 6.12 guest (NixOS) on Apple silicon, 8 cores, 15 GiB RAM, aarch64; rustc 1.99.1, plain `cargo` (no `mold`), `-j` default.

  | Measure (commands of D6) | Before | After |
  |---|---|---|
  | Clean `cargo test --workspace --no-run --timings` | 600 s | 490 s (-18 %) |
  | Warm, `touch crates/df-core/src/lib.rs`, `cargo test --workspace --no-run` | 359 s | 274 s (-24 %) |
  | Warm, `touch crates/df-web/src/lib.rs`, `cargo test -p df-web --no-run` | 123 s | 85 s (-31 %) |
  | `target/debug` after the clean build | 33 GB | 12 GB (-64 %) |
  | `target/debug/deps` / `incremental` after the two warm builds | 27 GB / 12 GB | 8.3 GB / 6.6 GB |
  | Test binaries above 50 MB in `deps/` (after the clean build) | 54 | 38 |
  | CPU time of all compile units (sum, `--timings`) | 2369 s | 1930 s |

  "After" is `[profile.dev]` of D7 alone; nextest changes no build. A panic in a test still prints `file:line` (checked with `RUST_BACKTRACE=1` on a failing test: `panicked at crates/df-core/tests/…rs:4:5`, frames with `at ./tests/…rs:4:5`).

  Decisions on the out-of-scope rows (task 1.4): the 36 integration test binaries take 662 s of the 1930 s CPU time after the change (34 %; 866 s of 2369 s before), more than any other group, and `--timings` does not split their compile and link time. One test binary per crate stays a backlog row of `product/iterations/roadmap.md` with that share. `mold` is rejected: the Linux arm64 build already links with `rust-lld` (`.cargo/cc-lld.sh`), and nothing here shows linking as the bottleneck. A shared `CARGO_TARGET_DIR` across worktrees is rejected: cargo locks the directory, so parallel builds of two worktrees would wait for each other, and branches would invalidate each other's artifacts.

  What the timings of the baseline show: 1073 compile units add up to 2369 s of CPU time, 1427 s (60 %) of it in the 57 test and test-harness units of workspace crates; the proc-macro crates (`sqlx-macros`, `prost-derive`, `serde_derive`, ...) add up to about 20 s and build scripts to 13 s. Proc macros are therefore not a large share, so no `[profile.dev.build-override]` (D7).
- **[Debugger sessions lose variables]** → `CARGO_PROFILE_DEV_DEBUG=true` for that shell; documented in `docs/development.md`.
- **[nextest output differs from `cargo test`]** Agents parse test output in their reports. → Skill `rust` shows the nextest commands for one test, one crate and the before-commit run, and keeps `cargo test` as the fallback where nextest is not installed (sandboxes without `.bazel_env`).

- **[A test logs out or changes the session]** No test logs out today (`grep -i logout crates/df-e2e` is empty). → A test that needs it must use its own `Session::start` + `login`, noted in skill `e2e` and `docs/demo.md`.
- **[A failure in one test now looks like a login failure in another]** If the shared session breaks, the next test re-logs in (D1 step 3) instead of failing on a stale cookie. → The re-login prints a `login: …` line to stderr (visible with `--nocapture`), so a run shows how often it happened.
- **[Checked queries need a database for `prepare`]** Regenerating `.sqlx/` needs PostgreSQL (container or local). → CI and the pre-push hook build offline from the checked-in metadata; only the implementer needs a database once.
- **[Speed gain smaller than expected]** → Tasks 2.1 and 2.4 measure the summed fixture start times before and after, and the result goes into this section.

  Measured on a kept kind cluster of the machine above (temporary elapsed-time output around `World::start`, removed again; `--test-threads=2`, all 35 fixture tests of 9 binaries):

  | | Before (login per test) | After (login per binary) |
  |---|---|---|
  | Dex form submissions per run | 37 (35 fixture tests, the login test's own, the watcher of `ui.rs`) | 10 (one per binary, plus the login test's own) |
  | Summed `World::start` | 13.0 s | 11.1 s (22.1 s over the two runs of `E2E_REPEAT=2`) |
  | Median `World::start` | 344 ms | 203 ms |
  | Longest `World::start` | 674 ms | 662 ms |
  | One run of the test step (`cargo test`, after the reset) | not recorded | 384 s and 386 s |

  On a warm cluster the gain is small: a login costs about 140 ms, the rest of `World::start` (Playwright driver, browser connection, context) stays, so the suite saves about 2 s of about 6.5 minutes. The cold first login after a cluster start (up to `LOGIN_TIMEOUT`, 30 s) is the case this helps most; the measurement does not cover it. Re-login path (task 2.4): with the `web_sessions` rows deleted before each of three sequential tests of `demo.rs`, the second and third test logged `the stored login is no longer accepted, logging in again`, submitted the Dex form again and passed.

## Migration Plan

None. Rollback is a revert.
