# Design

## Context

See proposal.md (Why). Current state, after `iter3-task-dependencies` and `iter3-webhooks`:

- **Base branch**: always `projects.base_branch`. It is read in three places:
  - `df_agent_channel::assign::load_run_context`, which feeds `Workspace.base_branch`;
  - `df_agent_channel::pr::try_ensure_draft`, as the PR base;
  - the gateway snapshot query in `df_db::egress`, as `GitRoute.base_branch`, used by push checks.
- **Tasks**: `parent_id` and the task transitions come from `iter3-task-dependencies` (D2, [ADR-0055](../../../docs/adr/0055-task-transitions-through-one-transition-function.md)): the pure `df_core::task_transition::transition` and the applier `df_db::apply_task_event`. A child's end raises `ChildEnded` on its parent, whose arm ignores it when the parent is `completed`, `cancelled` or `failed`. A cross-task event locks only the target's task row. `create_task` validates its input in one transaction under row locks. `tasks.pr_merged_at` and the merged-PR path through mark-done come from `iter3-webhooks` (D4).
- **Cancel**: `df_db::tasks::cancel_task` runs in one transaction. Lock order everywhere is run rows first, then the task row.
- **Prepare**: `crates/factory-agent/src/workspace.rs` fetches base and task branch and fast-forwards the task branch only to its own remote.
- **Plugins**: the WIT `pull-requests` interface has `create`, `find-open`, `get` and `update`. The host detects capability interfaces by export.
- **Instance config**: `df_core::config`, next to `runs.keepAliveSeconds`.

## Goals / Non-Goals

**Goals:**
- One function decides a task's base branch; workspace, Git route and PR never disagree.
- Every check (base task, limits, completion gate, merge) is a database query plus at most one forge call. No forge reads are needed to decide.
- No change to the agent protocol or the gateway protocol.

**Non-Goals:**
- Notices when a base branch moves: backlog ("Base-branch notices").
- Retargeting a PR whose base task was merged: the forge handles it (GitHub retargets when the base branch is deleted; Forgejo keeps the PR, and a human retargets or closes it). Rejected as Dark Factory logic.
- Chats' exemptions: `iter3-chats`.

## Decisions

### D1: Schema

One migration:
- **`tasks.base_task_id`**: `UUID NULL REFERENCES tasks(id)`, indexed. It is set only by `create_task` after the same-project check, and never updated.
- **`projects.subtask_merge`**: `TEXT NOT NULL DEFAULT 'human' CHECK (subtask_merge IN ('human','parent-agent'))`.

No stored effective values, no per-task merge setting (ADR-0034 revised).

### D2: Effective base branch

`df_db::tasks::base_branch(conn, task_id) -> String` is a recursive CTE:
1. Walk `base_task_id` while the current base task is `completed`.
2. Return `factory/task/<key>` of the first base task that isn't Done, or `projects.base_branch` when the chain ends.

All three readers call it: `load_run_context`, `try_ensure_draft` and the snapshot query. The snapshot query joins it per run as a `LATERAL` subquery, which keeps the query single-statement.

Alternative: store the base branch name on the task at creation. Rejected, because it can't follow a base task that gets merged later.

Cycles can't form: `base_task_id` references an existing task at creation and never changes.

- **Push checks**: the run's effective base branch counts as upstream (`push-checks`, "Checked commits"). A subtask based on its parent starts on commits that are not its own, among them the forge's merge commits of `merge_subtask_pr`, which carry the forge account's identity. Checking them against the agent identity would reject every push of the next iteration (found by the e2e suite). `TaskRefs` gets the run's `base_branch` (`GitRoute.base_branch`), and the walk stops at that ref like at the task's own branch.

### D3: Base branch creation at start

In the scheduler's provisioning (`df_orchestrator::base_branch::ensure_base_branch`, before the volume and sandbox), for every run of a task with `base_task_id` whose base task is not Done:
- call `pull-requests.create-branch(repo, <base task's branch>, base_branch(base task))`;
- treat `conflict` as "exists".

Any other error fails the start: the run ends `failed` (not `provisioning_failed`, which would auto-resume into the same error) with detail `base branch: <forge message>`. Every start asks, so a run that failed at the forge does not leave the next start (after a message) without the branch; the cost is one cheap forge call per start. A Done base task needs no branch: D2 falls back to its own base.

Alternative: create the branch at `create_task`. Rejected, because the task may be `awaiting_dependencies` for a long time, and ADR-0052 keeps such tasks free of branches.

### D4: Prepare fast-forward

In `workspace.rs`, after fetching, both for fresh and existing workspaces:
- if `git merge-base --is-ancestor <task tip> origin/<base>` holds and the tips differ, move the local task branch to `origin/<base>` and push it, as a fast-forward with the explicit refspec;
- this happens before the WIP restore.

It is a plain fast-forward, so the Git proxy's rules accept it and no protocol change is needed.

Tasks without a base task get the same behaviour against the default branch, which keeps a not yet started branch current (ADR-0041 consequence).

### D5: Open subtasks, completion gate and limits

`df_db::tasks::open_subtasks(conn, task_id) -> Vec<OpenSubtask { key, pr_url }>` is one query over `parent_id = $1`, matching the `task-tree` spec:

```sql
status NOT IN ('in_review','completed','failed','cancelled')
OR (base_task_id = parent_id AND status NOT IN ('completed','cancelled')
    AND (pr_number IS NOT NULL OR a `pr.sync` job with `open` for the task is pending))
```

The pending job closes the window between the run end (which wakes the parent) and the job that opens the PR. A limits value of 0 is rejected when the configuration loads (at least 1).

- **`complete_task`**: calls it before storing the summary. A non-empty result returns an `is_error` result and the turn continues. Mark done (human) doesn't call it.
- **Limits**: `create_task` checks the following under the existing transaction, after the parent row lock taken there:
  - depth: a recursive count of `parent_id` ancestors plus one, compared with `tasks.maxDepth`;
  - `open_subtasks(caller).len()` compared with `tasks.maxOpenSubtasks`.

  Config: `tasks.maxDepth` (u32, default 5) and `tasks.maxOpenSubtasks` (u32, default 10) in `df_core::config`, Helm values and the configuration reference.

Alternative: count subtasks ever created. Rejected, because it breaks long-lived chats.

### D6: Subtree cancel

`cancel_task` becomes:
1. Select the subtree: a recursive CTE over `parent_id`, keeping the root and the descendants not in `completed`, `failed` or `cancelled`. It is ordered by depth.
2. Lock all their run rows, then all task rows. This is the global order "runs, then task" for the whole set. A concurrent run end of a descendant holds only that descendant's run and task rows, and wakes its parent by locking only the parent's task row (ADR-0055). It either finishes first or waits, and no cycle forms.
3. Call `apply_task_event` with `Cancelled { via }` for each task, top-down (`via` is `None` for the root, the root's key for descendants). The applier's own locks are already held.

Because parents are already `cancelled` when their children's `NotifyParent` effects run, the `ChildEnded` arm returns an empty transition for them. Only the root's parent is woken, so no special case is needed.

The `Cancelled` arm rejects a `completed` or `cancelled` root, as today. For a descendant the subtree query already left those out. Each task gets its own `task.cancel` audit effect; descendants carry `{"via": "<root key>"}`.

Alternative: a `CancelChildren` effect of the `Cancelled` arm. It would lock children one by one while their parent is locked, so the "all run rows first" order of step 2 would be lost.

### D7: Merging subtask PRs

`merge_subtask_pr` lives in `crates/df-agent-channel/src/tools/delegation/merge.rs`, next to `create_task`. The checks read the subtask in one query (no lock held across the forge call); in this order, with one error message each:
1. the subtask is a subtask of the run's task;
2. `projects.subtask_merge = 'parent-agent'`;
3. `base_task_id` = the run's task;
4. status `in_review`;
5. the subtask has a PR.

Then the head SHA is read from the subtask's last accepted task-branch push (`git_pushes`), and `pull-requests.merge(pr, head-sha)` is called. A forge call doesn't belong inside a row lock.

On success:
- In a new transaction, lock the subtask's run rows and then its task row (the global order of D6) and read its status again. Unless it became `completed` or `cancelled` meanwhile, set `pr_merged_at` if it is null and run the mark-done path with the calling agent as actor (audit `task.done`; the actor identifies the agent, there is no separate reason). The mark-ready call is skipped as for the webhook path, and the effects of the `MarkedDone` event release dependents and wake the caller, as intended (ADR-0052). If the subtask can no longer be marked done, the tool still reports the merge with the PR URL and says why: a `completed` subtask is already done, a `cancelled` one stays cancelled (the merge event leaves it unchanged, so no webhook changes that).
- In the same transaction a `pr.sync` job with `open` is enqueued for the caller's task, which opens its PR (spec "PR after a subtask merge").

The subtask's PR is opened by a `pr.sync` job after the push, while the run end wakes the parent at once. A subtask based on its parent therefore counts as open (D5) while such a job is pending, and `merge_subtask_pr` answers "no pull request yet, call again" in that window.

A later merge webhook finds `pr_merged_at` set and does nothing (`iter3-webhooks` D4). The `call_id` store makes retries return the stored result.

Alternative: wait for the merge webhook. Rejected, because the handover would depend on webhooks (ADR-0052).

### D8: WIT and plugins

Both functions are added to the existing `pull-requests` interface, so the host's capability detection doesn't change:

```wit
/// Merges if the head is still `head-sha`, with the repository's merge style.
merge: func(id: pull-request-ref, head-sha: string) -> result<_, error>;
/// Creates `name` at the tip of `from`; `conflict` if `name` exists.
create-branch: func(repo: repo-ref, name: string, %from: string) -> result<_, error>;
```

- **Forgejo**:
  - Merge: `POST /repos/{o}/{r}/pulls/{n}/merge` with `Do` = the repository's `default_merge_style` (read through `GET /repos/{o}/{r}`) and `head_commit_id`.
  - Branch: `POST /repos/{o}/{r}/branches` with `new_branch_name` and `old_ref_name`.
- **GitHub**:
  - Merge: `PUT /repos/{o}/{r}/pulls/{n}/merge` with `sha` and `merge_method` from the repository flags, preferring `merge`, then `squash`, then `rebase`. A 405 (not mergeable) maps to `conflict`.
  - Branch: `POST /repos/{o}/{r}/git/refs` after resolving `from` with `GET /repos/{o}/{r}/git/ref/heads/{from}`. A 422 "Reference already exists" maps to `conflict`.

The spike "merging with the project's merge method" (`iteration-3.md` §7) is the first task and confirms these fields against Forgejo.

**Spike result (Forgejo 11.0.16, run in a container, SQLite, admin token):**
- `GET /repos/{o}/{r}` returns `default_merge_style` (`merge` on a fresh repository) and the `allow_*` flags.
- `POST /repos/{o}/{r}/pulls/{n}/merge` with `{"Do": "<default_merge_style>", "head_commit_id": "<sha>"}`:
  - merged: `200` with an empty body (not JSON);
  - moved head: `409 {"message": "head out of date"}`;
  - not mergeable (conflict, or mergeability still being computed on a fresh PR, message `Please try again later`): `405`;
  - already merged: `405` with an empty message, so it can't be told from a conflict. The plugin therefore reads the PR first (`GET .../pulls/{n}`) and answers `ok` for a merged PR with the same head, `conflict` for another head, `invalid-input` for a closed one.
- `POST /repos/{o}/{r}/branches` with `{"new_branch_name", "old_ref_name"}`: `201`; existing name `409 "The branch already exists."`; unknown `old_ref_name` `500 "object does not exist [id: ...]"`, which the plugin maps to `not-found`.
- Mapping: `405` and `409` are `conflict` in both plugins. GitHub was not run live: its fields are taken from the REST documentation. Its merge is `PUT` with `sha` and `merge_method`; its "Reference already exists" `422` is `conflict`. Both plugins read the PR before merging (as above); GitHub reads the repository flags for the method.

Alternative: a new `branches` capability interface. Rejected: host detection would change, and branch creation only exists for PR bases.

### D9: Web UI

- **Details column** (`df-web`): the existing `tabs` component (`scripts/tabs.ts`, from `iter3-artifacts`) holds the active tab; there is no second tab component and the active tab is not remembered. The tabs render server-side: Overview is today's markup plus the base task link, and Subtasks is a list. Both re-render on `TaskChanged` of the task or of any direct subtask; the subtask event is published to the parent too: every transition that changes the status of a task with a parent adds the effect `Publish(TaskChanged { parent })`, a rule in `transition` itself.
- **Chip**: the task list query adds two counts per row from a `LEFT JOIN LATERAL` over direct subtasks.
- **Project form**: a select with the two values and one hint line each.

### D10: Demo

- **Step 1**: the e2e scenario creates `DF-21` with `base_task: DF-20` and asserts the reviewer's workspace contains `DF-20`'s commit (fake agent `run` step `git log`).
- **Step 4**: a project with `subtask_merge: parent-agent`. The fake feature agent creates two iterations with `base_task` = itself, the second with `depends_on: done` on the first. The first iteration commits and completes, the feature agent calls `merge_subtask_pr`, and the second starts with the merge on its base.
- **Forge**: the e2e suite runs against a real Forgejo, which has the merge and branch endpoints; no fake forge endpoints are needed.

## Risks / Trade-offs

- [Base task merged while a dependent is open; the dependent's PR still targets the merged branch] → The forge retargets (GitHub) or the human closes it; the fallback in D2 keeps the workspace valid.
- [A reviewer that commits gets a PR into the implementation branch, which then blocks nothing: its parent is the coordinator, not the implementer] → Accepted (ADR-0041). The coordinator sees the PR in the child-ended message.
- [Merging uses a SHA from Dark Factory's push records; a push outside Dark Factory moves the head] → The forge rejects the merge (head mismatch) and the agent gets the error.
- [Prepare fast-forward pushes to the remote on every run for a task without own commits] → Only when the tips differ; it is a no-op otherwise.
- [The GitHub merge method is guessed from repository flags, not configured] → Fine for the demo forge (Forgejo uses the repository default). A project setting can follow if someone needs a different method.
- [The base branch counts as upstream: commits on it that passed only under a subtask's task-scoped exceptions, or that humans pushed there, are not checked again for every task based on that branch] → Accepted: that branch is those tasks' PR target and already holds the commits.
- [Subtree cancel locks the whole subtree] → Bounded by the limits (depth 5, 10 open subtasks per level), and cancel is rare.

## Migration Plan

- One forward migration (D1). Existing tasks get no base task, and existing projects get `subtask_merge: human`.
- **WIT**: plugins built against the old world fail to load, since the interface gained functions. Both in-tree plugins are rebuilt in the same change; pre-MVP, no external plugins exist.
- Regenerate `.sqlx/`. Forward-only, no down migration.
