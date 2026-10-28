## Context

See proposal.md for the motivation and the list of files. Constraints that shape the approach:

- `factory-agent` is a binary-only crate, so its tests cannot move to the crate's `tests/` directory: they use private items (`runner`, `outbox`, `workspace`, …). They stay in a `#[cfg(test)]` module of the binary.
- `factory-agent/src/tests.rs` already sits next to two sibling test files, `idle_tests.rs` (runner idle wait) and `test_harness.rs` (`pub mod`, PKI and mock control plane, also used by `tests/cli.rs`).
- Every Bazel crate target uses `srcs = glob(["src/**/*.rs"])`, so moved and new files need no `BUILD.bazel` change. The test data globs do: `df-k8s` lists `src/snapshots/*.snap`, `df-web` already uses `src/**/snapshots/*.snap`.
- insta stores a snapshot in `snapshots/` next to the source file that calls `assert_snapshot!`, named after the module path (`df_k8s__render__tests__pvc_default.snap`).
- Plain `cargo test --workspace` and Bazel must keep working (AGENTS.md).

## Goals / Non-Goals

**Goals:**
- No test file over about 1,500 lines and no inline test module over about 500 lines among the files listed in the proposal.
- Each test sits with the tests of the module it exercises, under a name that states the behavior.
- The same tests run before and after the change: same count, same assertions, same coverage.

**Non-Goals:**
- Rewriting tests, changing assertions or test strategy, or adding tests (except where a shared fixture needs its own check, see D3).
- Splitting production code (`run_session`, `dark-factory/src/main.rs`): recorded as follow-ups below.
- A check that enforces the file size thresholds of D6 (function length is enforced, see D7).
- Shortening the functions that exceed the function length limit today.

## Decisions

### D1: `factory-agent` tests become `src/tests/` with one file per exercised module

`main.rs` keeps `mod tests;`, which now resolves to `src/tests/mod.rs`. `mod.rs` holds the shared fixtures and declares the topic modules:

| File | Tests (current location in `tests.rs`) |
|---|---|
| `cli.rs` | CLI parsing, log format, `log_outbound` fields (lines 26–365) |
| `connection.rs` | mTLS connect, control plane restart and replay, leftover outbox, certificate rotation, outage, resend after reconnect, shutdown flush (366–585, 858–1663, 8417–8563), `discarded_outbox_notice`, expired credential requests (`r16`) |
| `outbox.rs` | sequence, ack trimming, backpressure, discard, run state (586–857), `finding_25`, `r1`, `r15`, `r26`, `r29`, `r49` |
| `git.rs` | config denylist, planted config, hooks, non-HTTPS remote, socket mode, credential helper (1664–2102), `finding_4`, `finding_18`, `r8_r9`, `r8_r24`, `r10`, `r23`, `r38`, `r52`, `r53` |
| `workspace.rs` | clone, prepare, push, finalize, secret files, token off the volume (2103–3716), `finding_6`, `finding_13`, `r3`, `r13`, `r41`, `r42` |
| `harness.rs` | session resume, permissions, prompt queue, hidden blocks (3717–4219), `r22` |
| `transcript.rs` | ACP translation (4220–4453), `finding_16` |
| `usage.rs` | session log usage, rate limits, budgets (4454–4568, 4778–4858), `finding_20`, `finding_21`, `r21`, `r25`, `r39` |
| `mcp.rs` | MCP server (4569–4777) |
| `runner.rs` | outcome priority and `run_session` scenarios (4859–6985), `r2`, `r51`, resumed session replay |
| `runner_idle.rs` | the current `idle_tests.rs`, moved unchanged |

Line numbers refer to commit `d635cce`; the implementer assigns tests added later by the same rule (the module the test exercises). Helpers used by one topic move with it; helpers used by several go to `mod.rs`.

Alternative: tests next to each module (`src/connection/tests.rs`, …), as `df-web` does. Rejected for `factory-agent`: most of these tests are scenarios across modules (connection + outbox + mock control plane, runner + workspace + harness), which have no single home, and the shared fixtures would need a separate module anyway. One `tests/` directory keeps fixtures and scenarios together and leaves production files untouched.

`test_harness.rs` stays where it is: it is `pub` and used by the integration test `tests/cli.rs`.

### D2: Tests named after review rounds are renamed after their behavior

`test_review_r38_erase_and_expiry_drop_cached_credentials` becomes `test_erase_and_expiry_drop_cached_credentials`. Where the remaining name does not state a behavior (`test_finding_4_git_credential_scoping`), the implementer names the asserted behavior (`test_credentials_are_scoped_to_the_task_repository`). The `test_` prefix stays, as in the rest of the crate. The review references are dropped; the review logs of the archived changes and the git history keep them.

### D3: A shared builder replaces the copied `run_session` setup

About ten scenarios repeat roughly 100 lines each: a temp directory, an upstream repository (`setup_test_remote_repo`), a script file for the fake agent, an `Assign` with workspace and ACP harness, an outbox, the channels and `RunDriverParams`. `tests/mod.rs` gets a builder that produces exactly this:

```rust
let scenario = RunScenario::new(script_yaml)      // temp dir, upstream repo, script file
    .continuation("sess-1", "summary")            // optional
    .tools(vec![...]);                            // optional
let (result, messages) = scenario.run(|msg, control| { /* answer tool calls */ }).await;
```

`run` takes the control plane reaction as a closure because the scenarios differ there (queue a prompt at the first `TurnStarted`, answer `complete_task`, send `Cancel`). It returns the run result and every message the runtime sent. Scenarios with a deviating setup (two runs on one data directory, cancel before `Assign`) keep their own setup; the builder is used only where it removes code. It gets a small test of its own only if it contains logic beyond assembling the existing pieces.

Alternative: leave the copies. Rejected: the copies are the main reason the runner tests are hard to read, and every new scenario copies another block.

### D4: Inline test modules over about 500 lines move to `<module>/tests.rs`

`#[cfg(test)] mod tests { … }` at the end of `session.rs` becomes `#[cfg(test)] mod tests;`, and the body moves unchanged to `session/tests.rs`. Module path, `use super::*` and access to private items stay the same. This is the layout AGENTS.md already names ("`mod tests;` pointing to a `tests.rs` file for larger suites") and `df-web` uses.

`render.rs` uses insta: its five snapshots move with `git mv` from `df-k8s/src/snapshots/` to `df-k8s/src/render/snapshots/`, keeping their file names (the module path `render::tests` does not change). The `data` glob of the `df-k8s` test target becomes `src/**/snapshots/*.snap`, as in `df-web`. A missing snapshot fails the test in CI (`INSTA_UPDATE=no`), so a wrong location cannot slip through.

### D5: `df-web` test files over about 1,500 lines become directories split by topic

`task_pages/tests.rs` becomes `task_pages/tests/mod.rs` (fixtures such as `task_form`, `task_in`, `human`) with:
- `create.rs`: composer and task creation
- `list.rs`: list rows, filters, project menu, empty states
- `detail.rs`: details column, runs, links, status and mark done
- `messages.rs`: follow-up and queued messages, editing
- `layout.rs`: workspace layout, phone sheet, tokens, accessibility, CSS rules

`egress/tests.rs` becomes `egress/tests/mod.rs` (`app_with_egress`, `add_host`, `forgejo_app`, …) with:
- `rules.rs`: allowlist entries and the rules page
- `log.rs`: log page, stream, filters and graph
- `flows.rs`: open and closed rows, backlog, durations, row limit
- `icons.rs`: host icons

The tests and their names stay as they are.

### D6: The convention is documented in AGENTS.md

The "File structure" section of AGENTS.md gets three rules: inline `mod tests` up to about 500 lines, then `<module>/tests.rs`; a test file beyond about 1,500 lines becomes a directory split by topic; tests are named after the behavior they check, never after a review round or finding. The thresholds are guidance, not a lint.

### D7: Function length is enforced with `clippy::too_many_lines`

The pedantic lint `clippy::too_many_lines` counts the code lines of a function (no comments, no blank lines). The limit is clippy's default of 100.

Configuration:
- `clippy.toml` at the workspace root: `too-many-lines-threshold = 100`. `cargo clippy` picks it up by itself.
- Root `Cargo.toml`: `[workspace.lints.clippy] too_many_lines = "warn"`. The pre-push hook runs clippy with `-D warnings`, so a warning blocks the push. All crates under `crates/` inherit the workspace lints. `plugins/forgejo` and `plugins/test-probe` declare their own `[lints]` (they allow `unsafe_code` for the wit-bindgen bindings) and get the same entry in their `[lints.clippy]`. `tools/xtask` is a standalone workspace that neither lint run covers; it stays out.
- Bazel (the lint run of CI, `--config=lint`) ignores Cargo's `[lints]`. `build:lint` in `.bazelrc` gets `--@rules_rust//rust/settings:clippy.toml=//:clippy.toml` (the root `BUILD.bazel` exports the file) and `--@rules_rust//rust/settings:clippy_flags=-Dwarnings,-Dclippy::too_many_lines`. `-Dwarnings` must be in the list: rules_rust adds its default `-Dwarnings` only while `clippy_flags` is empty, so without it CI would let every clippy warning pass.

Existing outliers, measured on 2026-10-28 with `cargo clippy --workspace --all-targets --all-features -- -W clippy::too_many_lines` (49 functions in total):

| Function (file:line) | Lines |
|---|---|
| `factory-agent/src/runner.rs:586` (`run_session`) | 743 |
| `fake-acp-agent/src/agent.rs:65` | 374 |
| `df-k8s/src/render.rs:101` | 289 |
| `factory-agent/src/connection.rs:340` | 277 |
| `factory-agent/src/workspace.rs:56` | 213 |
| `df-k8s/src/render.rs:420` | 200 |
| `df-agent-channel/src/session.rs:628` | 152 |
| `factory-agent/src/main.rs:39` | 150 |
| `df-web/src/egress/log/page.rs:29` | 149 |
| `factory-agent/src/workspace.rs:488` | 141 |
| `df-web/src/transcript/mod.rs:166` | 135 |
| `dark-factory/src/main.rs:340` | 120 |
| `df-db/src/tasks.rs:108` | 115 |
| `df-k8s/src/provider.rs:114` | 113 |
| `df-k8s/src/reconcile.rs:182` | 106 |

Each gets `#[expect(clippy::too_many_lines, reason = "…")]` naming the follow-up. `expect` instead of `allow`: it fails once the function is split, so a stale exception cannot remain. The line numbers shift with group 5 and later commits; the implementer re-runs the measurement.

The other 34 are test functions (27 in `factory-agent`, 3 in `fake-acp-agent`, 1 each in `df-agent-channel` and `df-core`, 2 in the inline tests of `df-k8s` `render` and `provider`). Scenario tests are long on purpose, so test modules opt out once at module level (`#![allow(clippy::too_many_lines, reason = "scenario tests set up and check a whole run")]` in the test file, or `#[allow]` on an inline `mod tests`), only in the modules that need it.

Alternatives: `clippy::cognitive_complexity` measures branching instead of length but is a nursery lint and known to be noisy; rejected. A file length check is no clippy lint and would need its own script with an allowlist for generated code (`crd.rs`); not now, D6 stays guidance.

## Risks / Trade-offs

- [A test is lost or duplicated in the move] → Compare the list of test names before and after for each crate (`cargo test -p <crate> -- --list`); the only difference allowed is the renames of D2, checked against a rename table in the commit message.
- [Moving and renaming in one commit hides changes in review] → One commit per crate; within `factory-agent`, the pure move first, then the renames, then the builder. Git's rename detection keeps the moved files reviewable.
- [Open branches touching these test files conflict] → Do it right after the open PR (`fix/transcript-resume-and-polish`) is merged and before the next feature branch starts on these crates; the moves are mechanical to redo.
- [A builder hides what a scenario sets up] → The builder only bundles setup that is identical today; everything a scenario checks stays visible in the test.
- [Container-backed tests (`df-db`, `df-agent-channel`, `df-web`) cannot run without a Podman socket] → Run them locally with the socket or rely on CI; a move without code changes cannot change their result except through a missing file, which compilation catches.

- [The Bazel lint flags silently weaken CI (missing `-Dwarnings`)] → Task 7.3 checks both directions: the lint run passes, and a deliberately long function and a deliberate ordinary warning fail it.
- [`#[expect]` on 15 functions becomes permanent] → Each `reason` names its follow-up; `expect` fails once the function is short enough, which forces removing the attribute.

## Migration Plan

No deployment impact. Each commit builds and passes `cargo test --workspace`, `cargo clippy --all-targets` and the Bazel test targets on its own. Rollback is reverting the commit.

## Follow-ups (not part of this change)

- `factory-agent/src/runner.rs`: `run_session` is one 850-line function (setup, ACP connection, turn loop, idle wait, finalize). Split it into steps, using the runner tests of D1 and D3 as the safety net.
- `dark-factory/src/main.rs`: about 970 lines of code with the configuration wiring of all subcommands.
- The other 14 production functions of the D7 table: split them and drop their `#[expect]`, most urgently `fake-acp-agent` `agent.rs:65`, `df-k8s` `render.rs:101` and `factory-agent` `connection.rs:340`.
- Test files of 1,000 to 1,500 lines: `df-web/src/config_pages/tests.rs`, `df-agent-channel/tests/agent_port.rs`, `df-gateway/src/proxy/tests.rs`.
