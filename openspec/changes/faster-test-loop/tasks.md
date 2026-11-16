# Tasks

No published docs change: nothing a user or operator sees or configures changes (contributor docs and skills only).

The build group comes first: its baseline must be measured before anything else changes, and every later group then builds faster.

## 1. Faster local builds and a smaller `target/`

- [ ] 1.1 Measure the baseline with the commands of design D6 (clean, warm low crate, warm leaf crate, `target/` size) on the current `main`; write the numbers into the table of design.md (Risks, "Compile-time numbers") with the machine (CPU, cores, OS).
- [ ] 1.2 Add `[profile.dev]` and `[profile.dev.package."*"]` to the root `Cargo.toml` (design D7). Verify a panic in a test still shows file and line (`RUST_BACKTRACE=1` on a failing test), and `cargo clippy --all-targets --all-features -- -D warnings` passes. If the 1.1 timings show proc macros as a large share, add `[profile.dev.build-override] opt-level = 3` and measure both.
- [ ] 1.3 Pin `cargo-nextest` (entry in `TOOLS` in `tools/xtask/src/main.rs`, `//:bazel_env` in `BUILD.bazel`, `cargo xtask update-tools`), add `.config/nextest.toml` (design D8) and a `just test` recipe (`cargo nextest run --workspace` then `cargo test --workspace --doc`). Verify `just test` passes with a container socket (or report the container-backed tests it could not run), and that `cargo xtask update-tools --check` is clean.
- [ ] 1.4 Measure again with the D6 commands, fill in the "after" columns, and decide the out-of-scope rows (one test binary per crate, `mold`) from the numbers: add them to the backlog of `product/iterations/roadmap.md` with the measured share, or mark them rejected in the proposal.
- [ ] 1.5 Update skill `rust` (`.agents/skills/rust/SKILL.md`, "Tests": nextest commands for one test, one crate and before commit; `cargo test` as fallback without `.bazel_env`) and `docs/development.md` (profile, `CARGO_PROFILE_DEV_DEBUG=true` for debugger sessions, `just test`, nextest in the tools list). Remove the backlog row "Measure compile time …" from `product/iterations/roadmap.md`.

## 2. Shared login per test binary

- [ ] 2.1 Record the e2e baseline: on a kept cluster (`E2E_KEEP=1`) note the summed time of `World::start` per test (temporary `info!` with the elapsed time); write the numbers into design.md, Risks, "Speed gain".
- [ ] 2.2 Add `Session::start_logged_in(env)` with the process-wide stored state in `crates/df-e2e/src/browser.rs` (design D1, D3). Unit-test the pure part (the URL check that decides "landed on the UI" vs. "on the login form") first; verify with `cargo test -p df-e2e --lib`.
- [ ] 2.3 Use it in `World::start` (`fixture.rs`) and for the watcher in `tests/ui.rs`; move the login test (`tests/demo.rs`) to its own `Session::start` + `login` (design D2). Verify on a kept cluster: the Dex form is submitted once per binary (count of the login log line) and the full suite passes with `E2E_REPEAT=2`.
- [ ] 2.4 Verify the re-login path: delete the `web_sessions` rows during a run of one binary (`psql` on `127.0.0.1:15432`) and check that the next test logs in again and passes. Measure again as in 2.1 and record the result in design.md.
- [ ] 2.5 Update `docs/demo.md` (fixture section: shared login, re-login, the login test's own session) and skill `e2e` (`.agents/skills/e2e/SKILL.md`: a test that logs out or changes the session uses its own `Session::start` + `login`). Remove the backlog row "Reuse the login state across e2e tests" from `product/iterations/roadmap.md`.

## 3. Checked e2e queries (TD-81)

- [ ] 3.1 Convert the runtime queries of `crates/df-e2e/src/db.rs` to `query!`/`query_scalar!`/`query_as!` (design D4); regenerate `.sqlx/` with `cargo sqlx prepare --workspace -- --all-targets`; verify `SQLX_OFFLINE=true cargo build -p df-e2e --tests` without `DATABASE_URL` and `cargo test -p df-e2e --test unlanded_lookup` (container-backed; report if no socket).
- [ ] 3.2 Remove TD-81 from `docs/tech_debt.md` (summary row and entry) and close [#123] in the PR description.

## 4. Small fixes and stale records

- [ ] 4.1 TD-111: `ReviewEvent` enum in `crates/df-e2e/src/forgejo.rs`, a unit test of its Forgejo strings, callers in `tests/webhooks.rs` updated; verify `cargo test -p df-e2e --lib` and `cargo build -p df-e2e --tests`.
- [ ] 4.2 TD-110: set `ALLOWED_HOST_LIST` with `--set` from `$release` and `$namespace` in `deploy/e2e/e2e-local.sh`, remove it from `deploy/e2e/forgejo-values.yaml`; verify the webhook tests (`tests/webhooks.rs`) pass on a fresh cluster.
- [ ] 4.3 Remove TD-94, TD-103, TD-110 and TD-111 from `docs/tech_debt.md`; reword TD-93 (second user exists as `World::reviewer`; open: asserting `member` in the agent's tool result, a third user for `outside`).
- [ ] 4.4 Remove the backlog row "CI job running the e2e suite" from `product/iterations/roadmap.md`.

## 5. Integration

- [ ] 5.1 Full run: `just e2e-local` with `E2E_REPEAT=2` passes; `just test`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete` and `openspec validate faster-test-loop` are clean.

## Workflow follow-up

- Archive the change after review.

[#123]: https://github.com/prskr/dark-factory/issues/123
