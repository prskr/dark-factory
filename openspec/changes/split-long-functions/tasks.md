# Tasks

Requires `lint-and-crate-guardrails`. Each group is one commit or a series of commits; the workspace stays green after each one.

For every function:
1. List the covering tests and add characterization tests for unreached branches (design D1).
2. Split it (design D2).
3. Remove its `#[expect(clippy::too_many_lines)]`.
4. Verify:
   - `cargo clippy -p <crate> --all-targets --all-features -- -D warnings` passes without the attribute;
   - the crate's tests pass (container-backed ones with the Podman socket, `-j2`, `--test-threads=2`);
   - the commit message names the steps.

## 1. factory-agent: `run_session`

- [ ] 1.1 List the tests covering `run_session` (runner, runner_runtime and runner_idle scenarios); add characterization tests for branches no scenario reaches; verify `cargo test -p factory-agent` passes
- [ ] 1.2 Extract the shared run state struct (design D3 step 1); verify the factory-agent tests pass
- [ ] 1.3 Extract setup and finalize (design D3 step 2); verify the factory-agent tests pass
- [ ] 1.4 Extract the turn loop and the idle wait, and remove the `expect` (design D3 step 3); verify `run_session` and every extracted step are at or under 100 lines and the factory-agent tests pass

## 2. factory-agent: the other functions

- [ ] 2.1 Split `connection.rs` `run_loop`; verify as described above
- [ ] 2.2 Split `workspace.rs` `prepare_workspace` and `finalize_workspace`; verify as described above
- [ ] 2.3 Split `main.rs` `main`; verify as described above, and `cargo test -p factory-agent --test cli` passes

## 3. fake-acp-agent

- [ ] 3.1 Split `agent.rs` `build_agent` into one builder per scripted handler; verify as described above, and the factory-agent runner tests that use the fake agent pass

## 4. df-k8s

- [ ] 4.1 Split `render.rs` `render_sandbox` and `render_network_policy`; verify as described above, and the insta snapshots are unchanged (`INSTA_UPDATE=no`, no `.snap.new`)
- [ ] 4.2 Split `provider.rs` `resolve_cluster_target` and `reconcile.rs` `reconcile_active_runs`; verify as described above

## 5. df-agent-channel

- [ ] 5.1 Split `session.rs` `persist_batch` and `tools.rs` `run_handler`; verify as described above, with the container-backed tests

## 6. df-db

- [ ] 6.1 Split `tasks.rs` `create_task` and `requests.rs` `decide`; verify as described above, with the container-backed tests; the `.sqlx/` metadata is regenerated if a query moved or changed and is otherwise unchanged

## 7. df-web

- [ ] 7.1 Move the Alpine component of `egress/log/page.rs` to `assets/egress-log.js` (design D4) with a test that the page references the script and the asset is served; verify as described above, and `bazel --bazelrc=.bazelrc.agents build //crates/df-web` embeds the asset
- [ ] 7.2 Split `transcript/mod.rs` `apply_event` into one handler per event kind; verify as described above

## 8. dark-factory

- [ ] 8.1 Move the sandbox backend wiring to `src/sandbox.rs` (design D5), as a move-only commit; verify `git diff --color-moved` shows only moved lines plus `mod`/`use` changes, and the dark-factory tests pass
- [ ] 8.2 Split `KubernetesBackend::start`; verify as described above

## 9. Integration

- [ ] 9.1 Verify that `grep -rn -A1 "#\[expect($" crates plugins | grep too_many_lines` lists no production function. Run `cargo build --workspace`, `cargo test --workspace` (crate by crate with the Podman socket), `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all --check`, the xtask checks and `bazel --bazelrc=.bazelrc.agents build --config=lint //crates/... //plugins/...`; verify all pass

## Workflow follow-up

- Implement `clippy-strictness` next.
