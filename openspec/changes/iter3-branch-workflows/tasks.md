# Tasks

These changes must be merged before this one:
- `iter3-webhooks`: routing, and a merge makes the task Done.
- `iter3-task-dependencies`: `create_task`.

Their specs (`forge-webhooks`, `factory-tools` "create_task", `pull-request-lifecycle` "Back to draft when the task reopens") must be archived so that the MODIFIED deltas here apply.

`iter3-task-tree` may land before or after this change; see group 8.

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Domain (`df-core`)

- [ ] 1.1 Write failing tests, then implement `ExistingBranch::parse(name, project_base)` (design D4). Verify with `cargo test -p df-core branch`. The tests cover:
  - valid names: `renovate/serde-1.x`, `fix/cve-2026`;
  - every rejected form: empty, more than 200 bytes, `refs/heads/x`, `-x`, `a..b`, `a@{b`, control characters, space, `~^:?*[\`, trailing `/` `.` and `.lock`, `a//b`;
  - the project base branch;
  - the `factory/` prefix.
- [ ] 1.2 Write a failing serde test, then add `RewritePolicy { Never, WithLease }` with `with-lease` as the default and the database text form. Verify with `cargo test -p df-core rewrite_policy`.

## 2. Storage (`df-db`)

- [ ] 2.1 Add the migration from design D1 and regenerate `.sqlx/`. Verify with a `df-db` migration test:
  - existing tasks are backfilled to `factory/task/<key>`;
  - existing projects get `with-lease`;
  - a second open task on the same `(project, branch)` is rejected, and one is accepted after the first is `cancelled`.
- [ ] 2.2 Write failing tests, then store `branch` (default `factory/task/<KEY>`), `base_branch` and `pr_attached` at task creation. A unique violation maps to `BranchHeld { task }`. Verify with `df-db` tests:
  - creation on a held branch returns the holder's key;
  - creation with an attached PR stores number, URL, base and the flag.
- [ ] 2.3 Write failing tests, then make reopening a final task through a queued message fail with `BranchHeld` when another open task holds the branch ("Reopening needs the branch"). Verify with `df-db` tests for both scenarios of that requirement.
- [ ] 2.4 Write failing tests, then add `projects.branch_rewrite_policy` to project create, edit and audit, and read the effective base branch from `tasks.base_branch` before `projects.base_branch` (design D2). Verify with `df-db` tests.

## 3. Contracts and forge plugins

- [ ] 3.1 Add `RewritePolicy` to `darkfactory.agent.v1.Workspace` and `darkfactory.gateway.v1.GitRoute`, and `existing_branch` to `Workspace` (design D5). Verify with `buf format` and `cargo build --workspace`.
- [ ] 3.2 Add `pull-requests.list-open(repo, filter)` to the integrations WIT. The filter record has `head` and the optional fields `head-prefix`, `author`, `label` and `base`; plugins return `invalid-input` for the fields that aren't supported yet (design D3). Implement it in `plugins/test-probe`. Verify with the plugin-host tests.
- [ ] 3.3 Write failing tests, then implement `list-open` by head in `plugins/forgejo`: the `head` query on v16 or later, otherwise a paginated list filtered on the plugin side; fork PRs are excluded. Verify with the plugin's tests against recorded responses covering the three spec scenarios.
- [ ] 3.4 Write failing tests, then implement `list-open` by head in `plugins/github` (`head=<owner>:<branch>`, paginated). Verify with the plugin's tests covering the three spec scenarios.

## 4. Git proxy (`df-gateway`)

- [ ] 4.1 Write failing tests, then build `GitRoute.task_branch` from `tasks.branch` and `GitRoute.rewrite_policy` from the project in the snapshot query. Verify with a `df-db` snapshot test for an existing-branch task.
- [ ] 4.2 Write failing tests, then apply the ancestry check in `check_refs` only under `never` (design D6). Verify with `cargo test -p df-gateway refs`, covering every scenario of the `git-proxy` delta. The lease case is a fake upstream rejecting a stale old value.

## 5. Runtime (`factory-agent`)

- [ ] 5.1 Write failing tests, then replace the `force: bool` of `Git::push` with `Force::{No, Always, WithLease(sha)}`, and persist `remote_sha` in the workspace state (design D7). Verify with `cargo test -p factory-agent git`.
- [ ] 5.2 Write failing tests, then push after a turn with lease under `with-lease` and plainly otherwise. A lease or non-fast-forward rejection maps to `REJECTED_NON_FAST_FORWARD`. Verify with `factory-agent` workspace tests against a local bare remote, covering every scenario of "Push after every turn with new commits", including the agent fetching on its own.
- [ ] 5.3 Write failing tests, then add the prepare rules:
  - an existing workspace with an unpushed rebase and an unchanged remote is pushed with lease, or reported diverged under `never`;
  - a fresh workspace with `existing_branch` checks out `origin/<branch>` or fails with `BranchMissing` (design D8).

  Verify with `factory-agent` workspace tests for every scenario of "Fresh workspace" and "Existing workspace".
- [ ] 5.4 Update `crates/factory-agent/docs/` (workspace and push behaviour, rewrite policy) and `design/agent-run.md` §6.2 and §6.4 (push with lease). Verify that the documented steps match the tests from 5.2 and 5.3.

## 6. Control plane (`df-agent-channel`)

- [ ] 6.1 Write failing tests, then fill `Workspace.task_branch`, `existing_branch` and `rewrite_policy` in `Assign` from the task and project. Verify with `cargo test -p df-agent-channel assign`.
- [ ] 6.2 Write failing tests, then add `branch` to `create_task`. The steps are: reject it together with `base_task`, validate it, call `list-open`, and attach the task or fail with several PRs or a forge error (design D3). Verify with `df-agent-channel` tool tests for every `create_task` and "Pull request attached at creation" scenario.
- [ ] 6.3 Write failing tests, then use `tasks.branch` and the effective base in PR creation, and record a PR found for an existing-branch task as attached. Verify with `df-agent-channel` PR tests for "Draft PR on first push ahead of base" and "Idempotent creation".
- [ ] 6.4 Write failing tests, then skip the PR changes for attached PRs: ready on completion, ready on mark done, and back to draft. `update_pull_request` fails for them. Verify with `df-agent-channel` tests that use a recording forge and assert no `update` call for attached PRs, while the summary comment is still posted.
- [ ] 6.5 Write failing tests, then map a `BranchMissing` prepare failure to a failed run with the reason, which makes the task `failed`. Verify with a `df-agent-channel` run-end test.
- [ ] 6.6 Write failing tests, then route webhook events by the stored branch (design D9). Verify with the `forge-webhooks` routing tests: a task PR, an attached PR, a branch no task has, and a final task that is the most recent holder.

## 7. Web UI (`df-web`)

- [ ] 7.1 Write failing tests, then add the rewrite policy select to the project form (design D10). Verify with `df-web` form tests for the default, a change, and the audit entry.
- [ ] 7.2 Write failing tests, then add "Existing branch" to the composer's options menu, with a chip and server-side errors that keep the composer open. Verify with `df-web` tests for the two composer scenarios.
- [ ] 7.3 Write failing tests, then show the "attached" badge next to the PR link in the task details. Verify with a `df-web` render test.

## 8. Integration

- [ ] 8.1 If `iter3-task-tree` is archived before this change:
  - read the base task's branch from `tasks.branch` in `base_branch()`, with `tasks.base_branch` before the project fallback;
  - skip "Following the base branch" for existing branches (design D8);
  - add the base-task scenario to the MODIFIED "Draft PR on first push ahead of base" block here.

  If it isn't, add these points to `iter3-task-tree`'s tasks instead. Verify with `openspec validate iter3-branch-workflows --strict` and the task-tree tests.
- [ ] 8.2 Extend the fake forge and the e2e suite (`crates/df-e2e`). Verify with `just e2e-local`. The test covers:
  - a task created on an existing branch with an open PR is attached, completes, keeps the PR's draft state and gets the summary comment;
  - a merge of that PR makes it Done;
  - a rebase pushed with lease under `with-lease` succeeds.
- [ ] 8.3 Write the how-to "Work on an existing branch" in `docs/` (fan-out with `create_task` and `branch`, attached PRs, rebasing with lease, what a lease failure means). Link it from the playbook §2.1. Verify that the steps match the e2e flow from 8.2.
- [ ] 8.4 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length` and `openspec validate iter3-branch-workflows --strict`. Verify that all four pass.

## Workflow follow-up

- Archive after review. Then the ADR-0025 and ADR-0033 amendments are fully implemented.
