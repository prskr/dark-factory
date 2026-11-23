# Design

## Context

State after `read-only-workspace-emptydir` (DF-9), which this change requires:

- **Read-only property**: `tasks.read_only_workspace` with `CHECK (read_only_workspace = (kind = 'chat'))`. DF-9 D1 says the follow-up relaxes it. The gateway route (`df-gateway-control/src/snapshot.rs`, `GitRoute.read_only` from `ActiveRun.read_only`), `Assign.workspace.read_only`, the `emptyDir` data volume and the session state restore follow the property. Chat-only behavior (tool subset `CHAT_TOOLS`, `CHAT_INSTRUCTIONS`, archiving, chat slots, chat keep-alive) stays on the kind.
- **Read-only prepare** (`prepare_read_only`, `crates/factory-agent/src/workspace.rs`): blobless clone (`--filter=blob:none --no-checkout`, all branches) or `git fetch origin`, then `checkout --force --detach origin/<base_branch>`, `reset --hard`, `clean -fdx`, hidden paths. It pushes nothing. On an `emptyDir` every new pod clones.
- **Base task** (`task-tree`): `create_task` takes `base_task`; `tasks.base_task_id`. A task's effective base branch is its base task's task branch, or the base task's own effective base branch once the base task is Done (`RunContext::base_branch()`). `Assign.workspace.base_branch` and the Git route use it. Before a run of a task whose base task is not Done, the control plane creates the base task's branch from its own base if it is missing. Chats can't be base tasks (`DbError::InvalidBaseTask`).
- So a read-only task with a base task already checks out the reviewed branch with today's `prepare_read_only`: `origin/<base_branch>` **is** the base task's branch. No checkout code changes.
- **Tools**: `tools.rs` filters by `rc.is_chat()` against `CHAT_TOOLS`; tasks get all of `factory-tools.v2.json`. `read_pull_request_feedback` and `comment_on_pull_request` act on "the task's pull request" and fail with "no pull request opened yet" without one.
- **Run end** (`record_run_report`): `completed` → `in_review`; the PR job posts the summary and marks the PR ready if the task has a PR. Push reports with commits ahead open a draft PR.
- **Forge webhooks** drop reviews and comments of the bot account (`forge-webhooks`, "Bot authored events dropped"). A reviewer's comment on the implementer's PR therefore never wakes the implementer; the parent relays with `send_message`, as today.
- **Notices**: the runtime writes transcript notices (`workspace.diverged`, `workspace.hidden_paths_removed`), shown in the task's transcript.

## Goals / Non-Goals

**Goals:**
- A reviewer can't change the reviewed branch or open a PR, enforced by the platform.
- Every run of a reviewer sees the current head of the reviewed branch, with full history for diff, merge base, log and blame.
- The transcript records which commit each run reviewed.
- Reuse DF-9 unchanged: one property, set at creation.

**Non-Goals:**
- Persona default for read-only tasks: iteration 4, "Full permission model (ADR-0011)", as per-persona permissions.
- Read-only tasks in the new-task composer and read-only tasks without a base task: the backlog row this change adds (open questions 1 and 2).
- Automatic summary comment on the base task's PR: rejected (D6).
- Fetching only two branches: rejected for now (D4).
- A file-system write block: rejected, as in DF-9.

## Decisions

### D1 — `read_only` on `create_task`, only with `base_task`

`create_task` gets `read_only: boolean` (default false). With `read_only: true`:
- `base_task` is required. Without it the result is an error: "`read_only` needs `base_task`: a read-only task reviews the branch of its base task". A read-only task without a base task would work on the project's base branch, like a chat; that is open question 2.
- `branch` stays incompatible with `base_task` (existing rule), so `branch` + `read_only` fails with the existing error.
- The base task must be a task with a writable workspace. A chat (existing check) or a read-only task (new) gives the existing "invalid base_task" error. The base task may be the caller's own task (a writer asks for a review of its own branch).
- The new task has kind `task`, `read_only_workspace = true`, task branch `''` (holds nothing, as a chat) and parent and creator as for every `create_task`.

Migration: replace DF-9's `CHECK (read_only_workspace = (kind = 'chat'))` with:
- `CHECK (kind <> 'chat' OR read_only_workspace)` — every chat is read-only;
- `CHECK (kind = 'chat' OR NOT read_only_workspace OR base_task_id IS NOT NULL)` — a read-only task has a base task;
- `CHECK (NOT read_only_workspace OR task_branch = '')` — a read-only task holds no branch.

`NewTask` (or `TaskPlacement`) gets `read_only: bool`; `create_task_on` writes it and checks the base task in the same statement that checks it today.

### D2 — Review checkout and the diff base

The checkout needs no runtime change: `Assign.workspace.base_branch` is the reviewer's effective base branch, which is the base task's task branch (`factory/task/DF-20`, or an existing branch such as `renovate/serde-1.x`). `prepare_read_only` fetches and checks out `origin/<that branch>` at every run start; a new pod clones it fresh. "Please review again" ends up in a new run (D5), so it sees the new commits.

The reviewer also needs the branch to compare against. The control plane knows it: the effective base branch of the base task (`main`, or the branch of the base task's own base task). New field:

```proto
message Workspace {
  // ...
  // A read-only review: the branch the reviewed branch (`base_branch`) is compared against,
  // the base task's own base branch. Empty for chats and writable workspaces.
  string review_base_branch = <n1>; // final number assigned at implementation
}
```

When `read_only` is true and `review_base_branch` is non-empty, the runtime after prepare:
- resolves `HEAD` and `git merge-base HEAD origin/<review_base_branch>` (empty if the ref is missing or there is no merge base);
- writes one info notice `workspace.review_checkout` naming the reviewed branch, the HEAD SHA, the diff base and the merge-base SHA, for example "Reviewing `factory/task/DF-20` at `3f2a9c1…` against `main` (merge base `9b0e4d2…`)".

The notice is a runtime transcript event, so the board member sees which commit each run reviewed. Chats send an empty `review_base_branch` and get no notice.

Base task Done (merged) before the reviewer runs again: its effective base branch moves to where it was merged (`task-tree`), so the reviewed branch and `review_base_branch` are the same, and the notice shows the same branch twice. The read-only instructions (D7) tell the agent the base task is done. No special case.

Base task never pushed: the control plane creates its branch from its base before the run (`task-tree`, existing). The review sees no commits ahead. The tool description of `create_task` recommends `depends_on: [{task, kind: completion}]` for reviews, as today.

Alternative: the control plane resolves the head SHA through the forge when building `Assign`. Rejected: a push between `Assign` and the fetch would make the recorded SHA wrong; only the runtime knows what it checked out.

### D3 — Gateway

No code change. DF-9 builds `GitRoute.read_only` from the property; a read-only task's route has no task and no WIP branch and every ref update is rejected with `df:read-only`. A test pins it for a task of kind `task` (task 3.2).

### D4 — Clone scope: all branches, blobless, not shallow

Kept as DF-9 and today: `clone --filter=blob:none --no-checkout` of all branches, no `--depth`. Commits and trees of the whole history are local, so `log`, `merge-base` and `diff <base>...HEAD` work offline; `blame` and `log -p` fetch the blobs they need on demand through the gateway.

Evaluated: `--single-branch --branch <reviewed>` plus a fetch of `review_base_branch`. It saves the commits and trees of other branches. For most repositories these share most of their history with the base branch, so the saving is small; the reviewer also loses other branches (for example to compare with an earlier attempt). Rejected for now; DF-9's risk "Clone on every new pod" asks for a measurement with a large repository, which decides whether to revisit it.

### D5 — No PR, Done on completion

- The control plane never opens a PR for a read-only task: the PR job skips push reports of such a task. None can arrive anyway (no task branch, gateway rejects); the check is the second line.
- Run end `completed` → task `completed` (Done) instead of `in_review`, with the same rules for undelivered messages (`queued`) and for cancelled tasks as today. `in_review` means "a PR waits for review"; there is none. Done also satisfies `depends_on` of kind `completion` and `done` at once, and the task stops counting as an open subtask.
- `task.child_ended`: one message, "completed". The task-delegation rule "Completion, then merge" (two messages) doesn't apply: there is no merge.
- No summary comment on any PR (D6) and no ready update.
- `complete_task` stores the summary; the parent gets it in the child-ended message.
- Mark done and cancel work as for any task (no PR, so the forge is not called).
- A message reopens the Done task (`queued`); the next run is a new run with a fresh checkout and the restored session (DF-9).

This is open question 3; the alternative is `in_review` as today, which needs a human to mark each review done.

### D6 — Pull request tools act on the base task's PR

For a read-only task "the task's pull request" is its base task's PR:
- `read_pull_request_feedback` reads it (with its own feedback cursor, stored on the reviewer task as today);
- `comment_on_pull_request` comments on it or replies in its threads;
- both fail with "no pull request opened yet" when the base task has none. There is no extra check for a closed or merged PR: the forge still takes comments there, and any forge error is passed on as a tool error (`Integration failures are tool errors`).

An attached base task PR (Renovate) takes comments, as for the task that holds it. Comments go through the bot account; the forge webhook drops them, so the implementer is not woken by them (existing rule). The parent relays.

The control plane does not post the reviewer's completion summary on the base task's PR: the reviewer writes its own comments, and a second copy would be noise.

### D7 — Tools and instructions

Offered to a read-only task: every tool of `factory-tools.v2.json` except:
- `update_pull_request` — the task has no PR of its own; the base task's PR belongs to the base task;
- `merge_subtask_pr` — only the base task of a subtask may merge; a read-only task is never a base task (D1).

Kept, with notes: `complete_task` (Done, D5), `comment_on_pull_request` and `read_pull_request_feedback` (base task's PR, D6), `read_ci_status`, `read_ci_log`, `list_pull_requests`, `comment_on_issue` (fails without a linked issue, as for any agent-created task), `ask_question`, `request_approval`, `create_task` (children may be writable), `send_message`, the artifact tools, `set_task_title`, `report_progress`, `list_personas`.

Implementation: a `ToolSet` choice in `assign.rs` from kind and property: `Chat` → `CHAT_TOOLS`; `ReadOnlyTask` → all minus `READ_ONLY_EXCLUDED_TOOLS`; `Task` → all. `tools.rs` uses the same choice for the "not available" error ("`update_pull_request` is not available in a read-only task").

Read-only instructions (`READ_ONLY_TASK_INSTRUCTIONS`, appended after the persona's system prompt, as for chats), filled with the branch names:
- the checkout is a detached, read-only copy of `<reviewed branch>` at the start of the run; pushes are rejected;
- compare against `origin/<review_base_branch>` (`git diff origin/<base>...HEAD`);
- a later message in the same run doesn't refresh the checkout: run `git fetch origin` and `git checkout --detach origin/<reviewed branch>` first;
- report with `comment_on_pull_request` on the reviewed PR, `write_artifact` and `complete_task`; work that needs commits goes into a task created with `create_task`;
- if the base task is Done, say so.

### D8 — Slots, keep-alive, volume

Read-only tasks have kind `task`: they count against `runs.maxConcurrent`, use `runs.keepAliveSeconds` (DF-9: keep-alive follows the kind) and are never released by `release_idle_chat_run`. The data volume is DF-9's `emptyDir`; volume retention skips them (DF-9).

A message in the keep-alive window continues the same run and checkout. This is rare for reviews: `complete_task` ends the run, and the usual "review again" arrives after it. The instructions (D7) cover the rest. Open question 4.

### D9 — Reset workspace and task page

- `POST /tasks/{key}/reset-workspace` returns 409 for a task with a read-only workspace, without an audit entry (DF-9 D8 leaves this to the follow-up). The action is already hidden by DF-9.
- The Overview tab shows "Read-only review of DF-20" (link) in place of "Base task DF-20".

## Risks / Trade-offs

- **Stale checkout within a run**: a message in the keep-alive window reviews the old checkout unless the agent refetches. Mitigation: instructions; open question 4 offers a platform fix.
- **Comments don't wake the implementer**: by design of the webhook filter. The parent relays. Unchanged from today's review flow.
- **Empty review**: a reviewer started before the base task pushed sees no diff. Mitigation: `depends_on` of kind `completion` in the tool description, as today.
- **Lazy blob fetches** for `blame` and `log -p` on large histories go through the gateway and can be slow. Accepted; same as chats.
- **Reviewer writes a fix anyway**: commits stay local and are lost at run end; the push is rejected with `df:read-only`, which the agent sees. The instructions point to `create_task`.

## Migration Plan

One migration after DF-9's: drop DF-9's `CHECK` and add the three of D1. Existing rows satisfy them (only chats are read-only). No data change. Rollback: an older version can't create read-only tasks; existing read-only tasks would violate DF-9's `CHECK`, so a rollback first cancels them and sets the property to false.

## Open Questions

1. **Human creation in the UI.** Should a board member be able to create a read-only task in the new-task composer? It needs a base task picker, which the composer doesn't have. *Recommendation*: not in this change. Board members ask a chat or a task agent to delegate the review; a composer option "Review of task …" goes into the backlog row of this change.
2. **Read-only without a base task.** Allow `read_only: true` without `base_task` (an analysis on the project's base branch, reporting through artifacts)? *Recommendation*: no, for now. Without a base task there is no PR for the PR tools and a chat covers the case. The backlog row names it; relaxing it later only drops a check.
3. **Status after completion.** Done at once (D5) or `in_review` as for other tasks? *Recommendation*: Done. There is no PR to wait for; `in_review` would need a human to mark every review done, and the parent's `depends_on` already treats it as finished.
4. **Fresh checkout within a kept-alive run.** Accept that a message in the keep-alive window keeps the old checkout (instructions tell the agent to refetch), or end read-only task runs after each turn (keep-alive 0) so every message gets a fresh checkout and a new `workspace.review_checkout` notice? *Recommendation*: accept (D8). `complete_task` already ends the run; a keep-alive of 0 costs a pod start, clone and session restore for every answer to a question.
5. **Marker text.** "Read-only review of DF-20" assumes every read-only task is a review. *Recommendation*: keep it; it is the only use until open question 2 changes.
