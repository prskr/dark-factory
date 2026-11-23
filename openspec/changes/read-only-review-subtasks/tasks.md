# Tasks

Requires `read-only-workspace-emptydir` (DF-9) to be merged first. Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`. Apply the answers to the open questions of `design.md` before starting; the tasks follow the recommendations.

## 1. Contract

- [ ] 1.1 Add `string review_base_branch` to `Workspace` in `agent.proto` with the comment of design D2. Assign the final number: the next free number of `Workspace` at implementation time, checked against its `reserved` entries. Replace the placeholder `<n1>` in `design.md` D2 and `proposal.md`. Verify with `cargo build -p df-proto`.
- [ ] 1.2 In `contract/mcp/factory-tools.v2.json`: add `read_only` (boolean) to `create_task` with a description (needs `base_task`; no branch, no PR; pushes rejected; reports through comments on the base task's PR, artifacts and `complete_task`; children may be writable; recommend `depends_on` of kind `completion`). Say in `comment_on_pull_request` and `read_pull_request_feedback` that a read-only task acts on its base task's PR. Verify with the tool schema tests (`cargo test -p df-agent-channel tool_`).

## 2. Database (`df-db`)

- [ ] 2.1 Write a failing migration test, then add the migration of design D1: drop DF-9's `CHECK (read_only_workspace = (kind = 'chat'))`, add `CHECK (kind <> 'chat' OR read_only_workspace)`, `CHECK (kind = 'chat' OR NOT read_only_workspace OR base_task_id IS NOT NULL)` and `CHECK (NOT read_only_workspace OR task_branch = '')`. Tests: a read-only task with base task and empty branch is accepted; without base task, with a branch, and a writable chat are rejected.
- [ ] 2.2 Write failing tests, then add `read_only` to the creation input of `create_task_on` (`NewTask` or `TaskPlacement`): it sets `read_only_workspace`, stores an empty task branch and rejects `read_only` without base task. Extend the base-task check to reject a base task with a read-only workspace (`DbError::InvalidBaseTask`). Tests: read-only task created; read-only base task rejected; chat base task still rejected. Regenerate `.sqlx/`. Verify with `cargo test -p df-db tasks`.
- [ ] 2.3 Write failing tests, then map run end `completed` to task status `completed` for tasks of kind `task` with a read-only workspace, keeping the undelivered-prompt, human-status and cancelled rules (spec `run-orchestration`, "Run end of a read-only task"). Tests: completed → `completed`; completed with an undelivered message → `queued`; `end_turn` → `waiting`. Check that the child-ended message is queued once and says "completed" (spec `task-delegation`). Verify with `cargo test -p df-db runs` and `cargo test -p df-agent-channel run_report`.

## 3. Control plane (`df-agent-channel`, `df-orchestrator`, `df-gateway-control`)

- [ ] 3.1 Write failing tests, then accept `read_only` in `create_task` (`tools/delegation.rs`): error without `base_task` (spec `factory-tools`, "create_task for a read-only task"), pass it to `create_task_on`. Tests: review subtask created with parent and base task; missing base task; `read_only` + `branch`; read-only task as base task. Verify with `cargo test -p df-agent-channel delegation`.
- [ ] 3.2 Write a failing test in `df-gateway-control/src/snapshot.rs`: a run of a task of kind `task` (key `DF-21`) with `read_only: true` gets a read-only route without task and WIP branch. No code change is expected (design D3). Verify with `cargo test -p df-gateway-control snapshot`.
- [ ] 3.3 Write failing tests, then add the tool set choice of design D7 (`Chat`, `ReadOnlyTask`, `Task`) in `assign.rs` and use it in `build_assign` and in the "not available" check of `tools.rs`. Tests: tool list of a read-only task; `update_pull_request` and `merge_subtask_pr` answered with a tool error that names "read-only task"; chat and task lists unchanged. Verify with `cargo test -p df-agent-channel assign` and `cargo test -p df-agent-channel tools`.
- [ ] 3.4 Write failing tests, then add `READ_ONLY_TASK_INSTRUCTIONS` (design D7) and `review_base_branch` to `build_assign`: the effective base branch of the base task (add `RunContext::review_base_branch()` in `df-orchestrator`, loaded with the run context). Tests: base task open (`factory/task/DF-20` against `main`); base task Done (`main` against `main`, instructions say it is done); chat (empty). Verify with `cargo test -p df-agent-channel assign` and `cargo test -p df-orchestrator run_context`.
- [ ] 3.5 Write failing tests, then let `read_pull_request_feedback` and `comment_on_pull_request` resolve the base task's PR for a read-only task (design D6). Tests: comment on base task's PR; reply in its thread; base task without PR gives "no pull request opened yet"; feedback cursor stored on the read-only task. Verify with `cargo test -p df-agent-channel feedback`.
- [ ] 3.6 Write failing tests, then skip PR opening, ready update and summary comment for read-only tasks in the PR job (`df-orchestrator`, design D5). Tests: push report of a read-only run opens no PR; completed read-only run makes no forge call. Verify with `cargo test -p df-orchestrator pull_request`.

## 4. Runtime (`factory-agent`)

- [ ] 4.1 Write failing tests with a local bare repository, then emit the `workspace.review_checkout` info notice after `prepare_read_only` when `review_base_branch` is non-empty (design D2): reviewed branch, full HEAD SHA, diff base and merge base (left out when there is none). Tests: notice content; no notice for an empty `review_base_branch`; a second prepare after a new commit on the reviewed branch checks out and names the new SHA; the clone is not shallow (`git rev-parse --is-shallow-repository` is `false`). Verify with `cargo test -p factory-agent workspace`.

## 5. Web (`df-web`)

- [ ] 5.1 Write a failing test, then answer `POST /tasks/{key}/reset-workspace` with 409 for a task with a read-only workspace, without an audit entry. Verify with `cargo test -p df-web reset_workspace`.
- [ ] 5.2 Write a failing test, then show "Read-only review of <base task link>" in the Overview tab for read-only tasks (spec `task-ui`). Verify with `cargo test -p df-web task_page`.

## 6. End-to-end

- [ ] 6.1 Add an e2e scenario (skill `e2e`) with the fake agent: a task creates a read-only review subtask of itself after a push; the reviewer's push is rejected with `df:read-only`; its comment lands on the base task's PR; it completes and becomes Done; the parent is woken once; a second message makes a new run whose `workspace.review_checkout` notice names the new head. Verify with `just e2e-local`.

## 7. Docs and roadmap

- [ ] 7.1 Update the published docs and verify with `just docs-build`:
  - `docs/content/using/delegation.md`: a "Read-only reviews" section: `create_task` with `base_task` and `read_only`, what the reviewer can and can't do, Done on completion, "please review again", writable children;
  - `docs/content/using/build-a-feature.md`: the reviewer step uses a read-only review subtask;
  - `docs/content/using/tasks.md`: status of a read-only task after completion, the Overview marker, no reset workspace;
  - `docs/content/security/git-proxy.md`: read-only tasks next to chats for `df:read-only`;
  - `docs/content/architecture/agent-runs.md`: review checkout and `review_base_branch`;
  - `crates/factory-agent/docs/configuration.md`: `Workspace.review_base_branch` and the notice `workspace.review_checkout`.
- [ ] 7.2 Update the backlog row of this change in `product/iterations/roadmap.md` to the answers of the open questions (follow-ups that stay open: composer option, read-only tasks without a base task, persona default in iteration 4's permission model).
- [ ] 7.3 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length` and `openspec validate read-only-review-subtasks --strict`.
