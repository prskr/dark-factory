# Proposal

## Why

A reviewer today gets a writable workspace, a task branch and a prompt that says "don't commit". The rule is only in the prompt: the QA persona (Grace Hopper) asked several times for "exceptions to the no-commit rule". With a read-only task, the platform enforces it: the gateway rejects every push, and every run starts from a fresh checkout of the branch under review.

This change builds on `read-only-workspace-emptydir` (DF-9). DF-9 adds the task property `read_only_workspace`, runs such tasks on an `emptyDir` with a restored harness session, and lets the gateway, `Assign` and volume choice follow the property. Today only chats set it. This change lets `create_task` set it for review subtasks and reuses that mechanism unchanged.

## What Changes

- **`create_task` argument `read_only`** (optional boolean, default false). `read_only: true` requires `base_task`; without it the call fails. The new task has kind `task` and the read-only workspace property set. A board member can't create a read-only task in the UI in this change.
- **Base task rules**: a read-only task can't be the base task of another task (same rule as for chats). A read-only task can have writable children, as a chat can.
- **Checkout**: the run checks out the branch of the base task (its effective base branch, `task-tree`), detached, freshly fetched at every run start. No shallow clone: the blobless clone of DF-9 stays, so `git diff <base>...HEAD`, `merge-base`, `log` and `blame` work. `Assign.workspace` gets a new field `review_base_branch` (the base task's own base branch). With it the runtime writes a notice `workspace.review_checkout` into the transcript: reviewed branch, reviewed commit SHA, diff base and merge base.
- **Gateway**: rejects every push of a read-only task, through DF-9's property. No gateway code change.
- **No branch, no PR of its own**: a read-only task holds no task branch and no WIP branch, and the control plane never opens a PR for it. It reports through `comment_on_pull_request` and `read_pull_request_feedback` on the **base task's** PR, through artifacts and through `complete_task`.
- **Lifecycle**: a run of a read-only task that ends `completed` makes the task Done (`completed`), not `in_review`. Its parent receives one `task.child_ended` message. A message (for example "please review again" through `send_message`) reopens it as for any Done task, and the next run checks out the new head.
- **Run slots**: read-only tasks are tasks: they count against `runs.maxConcurrent` and use the task keep-alive window, not the chat slots.
- **Tools**: the full task tool set minus `update_pull_request` and `merge_subtask_pr`. A call to one of them returns a tool error.
- **Instructions**: the control plane appends fixed read-only instructions to the persona's system prompt: the checkout is read-only, the reviewed branch and the diff base, how to report.
- **Reset workspace**: `POST /tasks/{key}/reset-workspace` answers 409 for a task with a read-only workspace (DF-9 hides the action; read-only tasks are served by the task pages, chats are not).
- **Task page**: the Overview tab shows "Read-only review of `<base task>`".
- **Docs**: delegation and review how-to, factory tools reference, task lifecycle, Git proxy security page.

Out of scope:
- **Persona default** ("this persona's tasks are always read-only"): iteration 4, "Full permission model (ADR-0011)" in `product/iterations/roadmap.md`, as part of per-persona permissions; named in the backlog row this change adds.
- **Read-only tasks from the new-task composer**, and **read-only tasks without a base task** (analysis on the project's base branch): the backlog row this change adds (open questions 1 and 2 in `design.md`).
- **Automatic PR comment with the reviewer's completion summary on the base task's PR**: rejected; the reviewer posts its own comments, the summary goes to the parent.
- **Fetching only the reviewed branch and its base**: rejected for now (design D4); revisit with the large-repository measurement of DF-9.
- **A file-system write block** in the checkout: rejected, as in DF-9; the gateway and the fresh checkout keep it read-only.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `factory-tools`: `create_task` takes `read_only`; tools of a read-only task; the PR tools act on the base task's PR.
- `task-tree`: a read-only task needs a base task and can't be one.
- `task-branches`: a read-only task holds no branch.
- `task-management`: the read-only workspace property is also true for read-only tasks (modifies DF-9's requirement); reset workspace rejected for them.
- `workspace-lifecycle`: the review checkout and the `workspace.review_checkout` notice.
- `pull-request-lifecycle`: no PR for a read-only task.
- `run-orchestration`: Assign of a read-only task (tools, instructions, `review_base_branch`); run end `completed` → Done.
- `task-delegation`: one child-ended message when a read-only child completes; writable children of read-only tasks.
- `task-ui`: read-only marker on the task page.

`git-proxy` and `chats` are unchanged: DF-9's "Read-only runs can't push" and chat requirements already cover read-only tasks.

## Impact

- Depends on `read-only-workspace-emptydir` (DF-9). Implement after it is merged.
- `contract/mcp/factory-tools.v2.json`: `read_only` on `create_task`; descriptions of `create_task`, `comment_on_pull_request`, `read_pull_request_feedback`.
- `contract/proto` (`agent.proto`): `Workspace.review_base_branch = <n1>` (final number set at implementation).
- `crates/df-db`: migration relaxing DF-9's `CHECK` on `tasks.read_only_workspace`; `create_task_on` sets it; base task check rejects read-only tasks; run end maps `completed` → `completed` for read-only tasks; `.sqlx/`.
- `crates/df-agent-channel`: `create_task` argument, tool filter, read-only instructions, `review_base_branch` in `Assign`, PR tools resolve the base task's PR.
- `crates/df-orchestrator`: no PR job for read-only tasks; the effective base branch of the base task for `review_base_branch`.
- `crates/factory-agent`: the `workspace.review_checkout` notice.
- `crates/df-web`: 409 on reset workspace; read-only marker in the Overview tab.
- `crates/df-e2e`: one review scenario.
- `product/iterations/roadmap.md`: one backlog row.
