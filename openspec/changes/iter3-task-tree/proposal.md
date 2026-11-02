# Proposal

## Why

`iter3-task-dependencies` lets agents create tasks, but every task still branches off the default branch and nothing bounds delegation. Pattern 2.2 needs a reviewer whose workspace contains the implementation ([ADR-0041](../../../docs/adr/0041-tasks-based-on-another-tasks-branch.md)). Pattern 2.3 needs iterations whose PRs target a feature branch and that the feature agent may merge ([ADR-0034](../../../docs/adr/0034-task-hierarchy-subtask-branches-and-merging.md), revised 2026-11-02). Without limits, a looping or prompt-injected agent can create tasks without bound.

## What Changes

- **Base task**:
  - `create_task` gets an optional `base_task` (key): a task of the same project that is not Done, failed or cancelled.
  - The new task's branch starts from the base task's branch, and its PR targets that branch. The value is fixed at creation.
  - An agent bases a subtask on its own branch by passing its own key.
  - There is no `subtask_base` setting.
- **Effective base branch**: the base task's task branch, or the project's base branch without a base task. Once the base task is Done, its own effective base branch is used instead, because a merged branch may be deleted. One resolution serves `Workspace.base_branch`, the gateway's `GitRoute.base_branch` and PR creation.
- **Base branch creation**: at the first start of a task with a base task, the control plane creates the base task's branch from that task's own effective base through the forge. A branch that already exists is fine.
- **Prepare fast-forward**: workspace prepare fast-forwards a task branch without commits of its own (its tip is an ancestor of `origin/<base>`) to `origin/<base>`.
- **PRs**:
  - Draft PRs target the effective base branch.
  - After a subtask PR is merged through `merge_subtask_pr`, the control plane opens the parent's draft PR if the parent has none, since its branch is now ahead.
- **Merging subtasks**:
  - Project setting `subtask_merge`: `human` (default) or `parent-agent`.
  - Factory tool `merge_subtask_pr(task)`. It is allowed only if:
    - the setting is `parent-agent`;
    - the caller is the subtask's parent;
    - the subtask's base task is the caller;
    - the subtask is `in_review` with a PR.
  - The control plane merges through the forge plugin with the head SHA it knows, then marks the subtask Done (`pr_merged_at`, mark-done path). Forge errors (conflict, failed required checks) come back as tool errors.
- **Completion gate**: `complete_task` fails while the task has open subtasks, and the error names them. A subtask is open if either:
  - its status is not `in_review`, `completed`, `failed` or `cancelled`; or
  - it is based on the task, has a PR, and is neither Done nor cancelled.

  Marking a task done by hand is not gated.
- **Cancelling a subtree**: cancelling a task also cancels every descendant that is not `completed`, `failed` or `cancelled`. This happens top-down in one transaction, so only the cancelled task's own parent is woken.
- **Limits**: instance settings `tasks.maxDepth` (default 5) and `tasks.maxOpenSubtasks` (default 10, counting open subtasks as above). Both are checked by `create_task`; exceeding one is a tool error.
- **Integrations WIT** (**BREAKING** for plugins, both in-tree plugins updated): `pull-requests.merge(id, head-sha)` and `pull-requests.create-branch(repo, name, from)` in the Forgejo and GitHub plugins.
- **UI**:
  - The details column gets tabs: Overview (today's details plus the base task link) and Subtasks (direct subtasks with status and their own done/total chip, plus a progress bar).
  - Task rows with subtasks get a done/total chip: done counts `in_review` and Done, total leaves out cancelled.
  - The task list stays flat.
  - The project form gets `subtask_merge`.
- **Docs**:
  - ADR-0034 and ADR-0041 (revised), the glossary terms Subtask and Base task, `iteration-3.md`, `roadmap.md` and the agent playbook (written);
  - the configuration reference for the two limits;
  - a how-to "Build a feature in iterations".

Out of scope:
- Questions to the parent task's agent: `iter3-parent-questions`.
- Exempting chats from the completion gate and subtree cancel: `iter3-chats`, which introduces chats.
- Notices to tasks when their base branch moves: backlog ("Base-branch notices").
- A parent picker in the composer and moving a task to another parent: backlog ("Re-parenting tasks").
- `base_task` and `depends_on` in the composer: backlog ("Dependencies set by humans").
- A per-task or per-subtree `subtask_merge` override: backlog. Adding it later is not a breaking change.
- Project-level overrides of the limits: backlog.
- A cap on messages relayed between parent and subtasks: rejected for now; depth and open-subtask limits bound fan-out, and ping-pong is visible in the transcript.
- `subtask_base` (`default-branch` / `parent-branch`) inherited along the tree: rejected (ADR-0034 revised); `base_task` covers it.
- Agents merging into the default branch or any branch that isn't a task branch: rejected (ADR-0034).
- A read-only mode that rejects a reviewer's pushes: backlog (ADR-0041).

## Capabilities

### New Capabilities
- `task-tree`: base tasks (`create_task` with `base_task`) and the effective base branch, base branch creation, open subtasks and the completion gate, cancelling a subtree, limits on depth and open subtasks, and merging subtask PRs by the parent's agent. The `complete_task`, `create_task` and cancel requirements stay where they are; `task-tree` adds to them, so this change doesn't conflict with `iter3-task-dependencies`' deltas of `factory-tools` and `task-management`.

### Modified Capabilities
- `workspace-lifecycle`: prepare fast-forwards a task branch without own commits to its base.
- `pull-request-lifecycle`: draft PRs target the effective base branch; a subtask merge into a task branch opens that task's PR.
- `factory-tools`: new tool `merge_subtask_pr`.
- `configuration-management`: project setting `subtask_merge`.
- `forgejo-integration`, `github-integration`: `merge` and `create-branch`.
- `task-ui`: Overview and Subtasks tabs, base task link, done/total chip on task rows.

## Impact

- `contract/wit/integrations/integrations.wit`: `pull-requests.merge`, `pull-requests.create-branch` (breaking for plugins).
- `plugins/forgejo`, the GitHub plugin: both functions, with tests against recorded responses.
- `contract/mcp/factory-tools.v2.json`: `base_task` on `create_task`, `merge_subtask_pr`.
- `crates/df-db`:
  - migration: `tasks.base_task_id`, `projects.subtask_merge`;
  - effective base branch resolution;
  - completion gate and limit queries;
  - subtree cancel;
  - `.sqlx/`.
- `crates/df-agent-channel`: `assign.rs` (effective base, branch creation at first start), `pr.rs` (PR base), tools (`create_task` arguments, `complete_task` gate, `merge_subtask_pr`).
- `crates/df-db/src/egress.rs` / `crates/df-gateway`: `GitRoute.base_branch` from the effective base.
- `crates/factory-agent/src/workspace.rs`: prepare fast-forward.
- `crates/df-web`: details tabs, chip, project form.
- Instance config and Helm values: `tasks.maxDepth`, `tasks.maxOpenSubtasks`.
- `crates/df-e2e`: demo step 4 (feature with two iterations, agent merge) and step 1–2 with `base_task`.
- Builds on `iter3-task-dependencies` (`parent_id`, `create_task`, mark-done hook) and `iter3-webhooks` (`tasks.pr_merged_at`).
