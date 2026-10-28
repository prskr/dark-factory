# Tasks

Every group is one commit and leaves the workspace green on its own. "Same tests" means: the output of `cargo test -p <crate> -- --list` before and after the group is identical, except for the renames of group 3, which the commit message lists as a table (old name → new name). Container-backed tests run with the Podman socket (`DOCKER_HOST`, see AGENTS.md) or in CI; report which ones could not run locally.

## 1. Convention

- [x] 1.1 Add the test organization rules of design D6 to the "File structure" section of `AGENTS.md` (inline `mod tests` up to about 500 lines, then `<module>/tests.rs`; test files over about 1,500 lines split by topic; tests named after behavior, never after a review round); verify the section reads consistently with the existing `mod tests;` bullet

## 2. factory-agent: split `tests.rs` (pure move)

- [x] 2.1 Record `cargo test -p factory-agent -- --list` as the baseline (in the scratchpad, not committed)
- [x] 2.2 Turn `src/tests.rs` into `src/tests/mod.rs` with the shared helpers and the topic modules of design D1 (`cli`, `connection`, `outbox`, `git`, `workspace`, `harness`, `transcript`, `usage`, `mcp`, `runner`, plus `workspace_finalize` and `runner_runtime`, split off to keep the files below about 1,500 lines); move `src/idle_tests.rs` to `src/tests/runner_idle.rs` and drop `mod idle_tests;` from `main.rs`; tests and names unchanged; verify the test list equals the baseline and `cargo test -p factory-agent` passes
- [x] 2.3 Verify no file under `src/tests/` exceeds about 1,500 lines (`wc -l`) and `bazel --bazelrc=.bazelrc.agents test //crates/factory-agent/...` passes

## 3. factory-agent: rename review-round tests

- [x] 3.1 Rename the 34 `test_review_r*` / `test_finding_*` tests after the behavior they assert (design D2); verify `grep -rnE "fn test_(review_r|finding_)[0-9]" crates/` finds nothing, the test list equals the baseline modulo the rename table, and `cargo test -p factory-agent` passes

## 4. factory-agent: shared `run_session` scenario builder

- [x] 4.1 Add `RunScenario` to `src/tests/mod.rs` (design D3: temp dir, upstream repository, fake-agent script, `Assign`, outbox, channels, `RunDriverParams`; `run` takes the control plane reaction and returns the result and all outbound messages)
- [x] 4.2 Use it in the `run_session` scenarios whose setup it reproduces exactly (outcome scenarios, runtime timings, `complete_task`, resumed session replay, idle wait tests); keep the own setup of deviating scenarios; verify the test list equals the post-rename list, `cargo test -p factory-agent` passes, and `src/tests/runner.rs` shrinks (report before/after line counts)
- [x] 4.3 `cargo clippy -p factory-agent --all-targets -- -D warnings`; verify no warning, in particular no unused helper left behind

## 5. Inline test modules to `<module>/tests.rs`

- [x] 5.1 `df-agent-channel`: move the test modules of `src/session.rs` and `src/scheduler.rs` to `src/session/tests.rs` and `src/scheduler/tests.rs` (design D4); verify the test list of `df-agent-channel` is unchanged and its tests pass (container-backed)
- [x] 5.2 `df-k8s`: move the test modules of `src/provider.rs`, `src/reconcile.rs` and `src/render.rs`; `git mv` the five `df_k8s__render__tests__*.snap` from `src/snapshots/` to `src/render/snapshots/`; change the test target's `data` glob in `crates/df-k8s/BUILD.bazel` to `src/**/snapshots/*.snap`; verify the test list is unchanged, `cargo test -p df-k8s` passes with `INSTA_UPDATE=no`, no `.snap.new` file appears, and `bazel --bazelrc=.bazelrc.agents test //crates/df-k8s/...` passes
- [x] 5.3 `df-db`: move the test module of `src/tasks.rs` to `src/tasks/tests.rs`; verify the test list is unchanged and the tests pass (container-backed)

## 6. df-web: split the large test files by topic

- [x] 6.1 `src/task_pages/tests.rs` → `src/task_pages/tests/{mod,create,list,detail,messages,layout}.rs` (design D5); verify the test list of `df-web` is unchanged and its tests pass (container-backed)
- [x] 6.2 `src/egress/tests.rs` → `src/egress/tests/{mod,rules,log,flows,icons}.rs`; verify the test list is unchanged, the tests pass, and no file of either directory exceeds about 1,500 lines

## 7. Function length lint

- [x] 7.1 Add `clippy.toml` (`too-many-lines-threshold = 100`) at the workspace root, `too_many_lines = "warn"` under `[workspace.lints.clippy]` and in the `[lints.clippy]` of `plugins/forgejo` and `plugins/test-probe` (design D7); verify `cargo clippy --workspace --all-targets --all-features -- -D warnings` reports the production functions of the D7 table (re-measured after groups 2–6) plus test functions, and nothing else
- [x] 7.2 Add the module-level `allow` with a reason to the test modules that need it and `#[expect(clippy::too_many_lines, reason = "…")]` to each production outlier; verify `cargo clippy --workspace --all-targets --all-features -- -D warnings` is clean and `grep -rn -A1 "#\[expect($" crates plugins | grep too_many_lines` lists exactly the re-measured production outliers
- [x] 7.3 Export `clippy.toml` in the root `BUILD.bazel` and add `--@rules_rust//rust/settings:clippy.toml=//:clippy.toml` and `--@rules_rust//rust/settings:clippy_flags=-Dwarnings,-Dclippy::too_many_lines` to `build:lint` in `.bazelrc`; verify `bazel --bazelrc=.bazelrc.agents build --config=lint //crates/... //plugins/...` passes, and fails with a temporary 101-line function and, separately, with a temporary unused variable (both reverted)
- [x] 7.4 Document the 100-line limit, the `expect`-not-`allow` rule and the test-module opt-out in the "File structure" section of `AGENTS.md`, next to the rules of task 1.1; verify the text matches `clippy.toml` and design D7

## 8. Integration

- [x] 8.1 Run `cargo build --workspace`, `cargo test --workspace` (with the Podman socket), `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all --check`, `cargo machete` and `bazel --bazelrc=.bazelrc.agents test //...`; verify all pass and the total number of tests in the workspace equals the baseline across all crates

## Workflow follow-up

- Archive the change after review.
- Propose the follow-ups of design.md ("Follow-ups") as separate changes: `run_session` split, the other functions with an `#[expect(clippy::too_many_lines)]`, `dark-factory/src/main.rs`, the 1,000–1,500 line test files.
