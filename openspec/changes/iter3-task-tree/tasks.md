# Tasks

Prerequisites: `iter3-task-dependencies` (`parent_id`, `create_task`, the task transitions `transition` / `apply_task_event` of ADR-0055, `tasks/delegation.rs`) and `iter3-webhooks` (`tasks.pr_merged_at`, merged-PR path) are merged.

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Forge: merge and branch creation

- [ ] 1.1 Spike against Forgejo:
  - merging a PR with `Do` = the repository's `default_merge_style` and `head_commit_id`, including the response for a moved head and for a conflict;
  - creating a branch from a ref.

  Record the results in `design.md` D8. Done when D8 names the confirmed fields and status codes.
- [ ] 1.2 Add `merge` and `create-branch` to `pull-requests` in `contract/wit/integrations/integrations.wit` (design D8). Verify that `bazel --bazelrc=.bazelrc.agents build //contract/...` and `cargo build --workspace` pass.
- [ ] 1.3 Write failing tests against recorded responses, then implement both functions in `plugins/forgejo`. Verify with the plugin's tests for every scenario of `forgejo-integration` "Merge a pull request" and "Create a branch".
- [ ] 1.4 Same for the GitHub plugin: merge-method choice from the repository flags, 405 → `conflict`, 422 "Reference already exists" → `conflict`. Verify with the plugin's tests for every scenario of `github-integration` "Merge a pull request" and "Create a branch".
- [ ] 1.5 Expose both functions through the host's forge client in `df-agent-channel`, and add them to the e2e fake forge. Verify with a host test that calls `merge` on a loaded Forgejo component against a stub server.

## 2. Schema and base branch

- [ ] 2.1 Add the migration of design D1 (`tasks.base_task_id`, `projects.subtask_merge`) and regenerate `.sqlx/` (`docs/development.md`, "Updating `sqlx` Offline Metadata"). Verify that `cargo build --workspace` passes offline and a `df-db` migration test shows existing projects with `subtask_merge = 'human'`.
- [ ] 2.2 Write failing `df-db` tests, then implement `base_branch(conn, task_id)` (design D2). Verify with `cargo test -p df-db base_branch` covering the scenarios of `task-tree` "Effective base branch", including a chain of two Done base tasks.
- [ ] 2.3 Use `base_branch` in `load_run_context`, `try_ensure_draft` and the gateway snapshot query. Verify with tests:
  - a `df-agent-channel` assign test: `Workspace.base_branch` is `factory/task/DF-20`;
  - a `pr.rs` test for "Push of a task with a base task";
  - a `df-gateway` control test: the route's base branch is the base task's branch.
- [ ] 2.4 Write failing tests, then add `base_task` to `create_task` (contract JSON schema and `delegation.rs`). Verify with `df-agent-channel` tool tests for every scenario of `task-tree` "Base task at creation".

## 3. Workspace

- [ ] 3.1 Write failing `factory-agent` workspace tests (local bare remotes in `tempfile` dirs), then implement the prepare fast-forward (design D4). Verify with `cargo test -p factory-agent workspace` covering every scenario of `workspace-lifecycle` "Following the base branch".
- [ ] 3.2 Write a failing test, then create the base task's branch on the first run (design D3). Verify with `df-agent-channel` tests for both scenarios of "Base branch created on first start", plus a forge error that ends the run `failed` with the forge's message.
- [ ] 3.3 Update `design/agent-run.md` §6.2 (prepare fast-forward, base branch resolution). Done when the section matches D2 and D4.

## 4. Tree rules

- [ ] 4.1 Write failing `df-db` tests, then implement `open_subtasks` (design D5). Verify with `cargo test -p df-db open_subtasks` covering every scenario of `task-tree` "Open subtasks".
- [ ] 4.2 Gate `complete_task` on `open_subtasks`. Verify with `df-agent-channel` tool tests for every scenario of "Completion blocked by open subtasks". The test for mark done covers that it isn't gated.
- [ ] 4.3 Add `tasks.maxDepth` and `tasks.maxOpenSubtasks` to `df_core::config`, the Helm values and the configuration reference (`docs/`), then check them in `create_task`. Verify with:
  - `df-core` config tests for the defaults;
  - tool tests for every scenario of "Delegation limits";
  - `helm unittest` for the rendered values.
- [ ] 4.4 Write failing tests, then add `Cancelled { via }` and the parent `TaskChanged` rule to `transition` (unit tests in `df-core`), and extend `cancel_task` to the subtree (design D6). Verify with `cargo test -p df-core task_transition` and `cargo test -p df-db cancel` covering:
  - every scenario of "Cancelling a subtree";
  - a concurrent run end of a descendant (two transactions, both orders) without deadlock.

## 5. Merging subtasks

- [ ] 5.1 Add `subtask_merge` to the project form, list query and audit. Verify with `df-web` tests for both scenarios of `configuration-management` "Subtask merge policy of a project".
- [ ] 5.2 Write failing tests, then implement `merge_subtask_pr` (contract JSON schema, `delegation.rs`, design D7). Verify with `df-agent-channel` tests with a fake forge, covering:
  - every scenario of `task-tree` "Merging subtask PRs" and "Merge outcome";
  - `factory-tools` "merge_subtask_pr";
  - `pull-request-lifecycle` "PR after a subtask merge";
  - a later merge webhook that changes nothing.
- [ ] 5.3 Update the agent playbook's tool names if they changed during implementation. Add the how-to "Build a feature in iterations" (project setting, `base_task`, `done` dependencies, `merge_subtask_pr`). Done when the how-to's steps match the e2e scenario of 7.1.

## 6. Web UI

- [ ] 6.1 Write failing `df-web` tests, then add the Overview and Subtasks tabs (design D9, component script `assets/details-tabs.js` registered in `COMPONENT_SCRIPTS`), including the `TaskChanged` publication for the parent. Verify with tests for every scenario of `task-ui` "Details tabs".
- [ ] 6.2 Add the done/total chip to task rows. Verify with `df-web` tests for both scenarios of "Subtask progress on task rows".
- [ ] 6.3 Check both at 390 px and 1280 px in a browser (`just serve-local`, if a container socket exists). Done when there is no horizontal scrolling and the tabs are touch-sized; otherwise state that the check was skipped.

## 7. Integration

- [ ] 7.1 Extend `crates/df-e2e` with demo step 1 using `base_task` (the reviewer's workspace contains the implementation commit) and demo step 4 (design D10). Verify with `cargo test -p df-e2e` against the local stack.
- [ ] 7.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length`, `cargo machete` and the workspace tests that need no containers. Done when all pass; list the container-backed tests that could not run.
