# Design

## Context

See proposal.md (Why) and `docs/tech_debt.md` (TD-11, TD-12, TD-13, TD-82, TD-91, TD-108).

Current state in code:

- `Session::run_effects` (`crates/df-agent-channel/src/session.rs`) runs `pr::on_push_report` and `pr::on_run_end` after the batch transaction committed, in `spawn_forge_work` bounded by `FORGE_TIMEOUT` (60 s). A failure is logged; nothing retries it except the next push report or run end.
- `pr::try_ensure_draft` takes a per-task advisory lock in a transaction held across up to three forge calls (find, create, find after `Conflict`), then `record_pr` (`UPDATE … WHERE pr_number IS NULL`). It returns the PR it created even when another caller recorded a different one (TD-13).
- `on_run_end` tries to open a PR when `pushed || completed` (TD-12), then marks it ready, writes the `pr-ready:<n>` transcript event and posts `tasks.completion_summary`, which `complete_task` sets and nothing clears (TD-11).
- Mark done (`task_done` in `crates/df-web/src/task_pages.rs`) calls `mark_ready_bounded` inline after `df_db::mark_task_done` committed.
- `iter3-webhooks` added the outbox job `pr.mark_draft {task_id}` (`MARK_DRAFT_KIND`, `MarkDraftJob` in `df-db/src/tasks.rs`, enqueued by `queue_message_tx`), handled by `MarkDraftHandler` (`crates/df-agent-channel/src/pr/mark_draft.rs`), registered in `crates/dark-factory/src/lifecycle_events.rs`. The dispatcher (`df-db/src/outbox/dispatcher.rs`) runs on the leader, at most `MAX_IN_FLIGHT` (8) jobs at once, at least once.
- `end_run_tx` (`crates/df-agent-channel/src/runs.rs`) is the single place a run ends, for runtime reports and control plane failures. `RunEnd` carries `task_branch_sha` but not `task_branch_pushed`.

## Goals / Non-Goals

**Goals:**
- Every PR forge action is an outbox job enqueued in the transaction that records its trigger.
- The session no longer calls the forge.

**Non-Goals:**
- Changing `pr.mark_draft`: it already follows this pattern.
- Per-task job ordering in the dispatcher: D3 makes the order irrelevant. Rejected.

## Decisions

### D1: One job kind `pr.sync`

Payload: `{ task_id, run_id: Option<RunId>, open: bool, ready: Option<DateTime<Utc>> }`. `open` asks to open the draft PR if the task has none; `ready` asks to mark the PR ready, and its value is the time of the completion (D3). `run_id` names the run whose transcript gets the forge events and whose summary is posted; mark done has none.

The handler (`crates/df-agent-channel/src/pr/sync.rs`, next to `mark_draft.rs`), on every attempt:

1. Loads task and project. Gone, or `cancelled` → done.
2. `open` and no PR recorded → today's `try_ensure_draft` under the advisory lock (find, closed-status check, create, `Conflict` → find), then `record_pr`. Afterwards it re-reads `pr_number` and continues with the recorded PR, never the one it created (TD-13).
3. `ready` set and a PR recorded → mark ready unless the task was reopened (D3); with `run_id`, append `pr-ready:<n>` to its transcript (already keyed, so idempotent); with `run_id`, post `runs.completion_summary` of that run if non-empty.
4. `ready` set and no PR recorded → `JobError::Retry` while an unfinished `pr.sync` job with `open` exists for the task (an earlier run's push whose PR creation is still retrying, while this run did not push); otherwise done, the task has no PR to mark.
5. A forge or database error → `JobError::Retry`; an invalid payload → `GiveUp`.

Retry policy as `pr.mark_draft` (30 s base, 10 min cap, give up after 1 h). Lease 3 min: up to five forge calls at the default plugin call timeout of 30 s.

Alternative: three kinds `pr.ensure_draft`, `pr.mark_ready`, `pr.post_summary` (as TD-91 sketched). Rejected: "mark ready" needs the PR that "ensure draft" opens, and the dispatcher runs jobs in parallel, so the kinds would have to wait for each other. One job whose steps are idempotent avoids the ordering.

### D2: Enqueue points

| Trigger | Transaction | Payload |
| --- | --- | --- |
| Task-branch push `Ok`, commits ahead > 0, task has no PR | the session's batch transaction (`apply` on `PushReport`) | `open`, `run_id` |
| Task-branch push `RejectedNonFastForward` | same; sets `diverged` there instead of `mark_diverged` afterwards | — |
| Run end with `task_branch_pushed` or outcome `completed`, task not `cancelled` | `end_run_tx` | `open = task_branch_pushed`, `ready = completed ? now()`, `run_id` |
| Mark done, task has a PR | `mark_task_done` | `ready = now()`, no `run_id` |

`RunEnd` gets `task_branch_pushed` from the report's `WorkspaceState`; `RunEnd::failure` sets it `false`. A completed run that never pushed gets `open = false`, so no PR is opened (TD-12); a PR an earlier run opened is still marked ready.

`PR_SYNC_KIND` and `PrSyncJob` live next to `MARK_DRAFT_KIND` in `df-db`. `Effects::pushes`, `spawn_forge_work`, `FORGE_TIMEOUT`, `pr::on_push_report`, `pr::on_run_end`, `pr::ensure_draft` and df-web's `mark_pr_ready`/`mark_ready_bounded` go away.

### D3: A reopened task is not marked ready

A ready job may run after a message reopened the task (forge down at completion, review arrives, `pr.mark_draft` runs first). The handler skips "mark ready" when a message of the task was queued after the job's `ready` time; it still posts the summary. A message pending at completion was queued before that time, so "Message waiting at completion" still marks ready.

Both handlers take the per-task advisory lock (`task-pr:<id>`, the one of step 2) around their check and forge update: `pr.sync` for "mark ready", `pr.mark_draft` for its status check and draft update. Without it, a message committed after the ready check lets `pr.mark_draft` see the PR still a draft and skip, and the late ready update leaves a reopened task's PR ready. With the lock, whichever handler runs second sees the other's result: a later `pr.mark_draft` finds the PR ready and drafts it, a later ready step finds the reopening message and skips. `pr.mark_draft` already skips tasks that finished again.

Alternative: serialize all `pr.*` jobs of a task in enqueue order. Rejected: needs dispatcher changes, and the lock is already there for step 2.

### D4: Completion summary per run

Migration: `runs.completion_summary TEXT NOT NULL DEFAULT ''` (values are not copied). `complete_task` writes it on its run. `tasks.completion_summary` stays, unused, so old replicas keep working during the rolling update (ADR-0012, expand/contract); a later change drops it (Workflow follow-up in tasks.md). The handler reads it by `run_id`, so a run without its own `complete_task` posts nothing (TD-11).

Alternative: keep the column on `tasks` and clear it after posting. Rejected: clearing races with the next run's `complete_task`.

### D5: Advisory lock bound by the dispatcher (TD-82)

Only outbox jobs (`pr.sync`, `pr.mark_draft`) take the per-task lock now, and the dispatcher runs at most `MAX_IN_FLIGHT` (8) jobs, so at most 8 pooled connections wait or hold it. The `ponytail:` comment at the lock names this ceiling.

### D6: Outbox wiring in its own module (TD-108)

`crates/dark-factory/src/outbox.rs` builds the handler list (`MarkDraftHandler`, `PrSyncHandler`, the lifecycle `DeliverHandler` when enabled) and starts the dispatcher; `lifecycle_events.rs` keeps only the lifecycle event handler.

## Risks / Trade-offs

- [The forge accepts the summary comment but the call fails] → The retry posts it again. Accepted (proposal, Out of scope).
- [A plugin call timeout above ~36 s lets the lease expire mid-job] → The job runs twice; the advisory lock, the recorded-PR check and the idempotent ready call keep the PR state right; only the summary may be posted twice.
- [A PR action is given up after an hour] → Logged and counted (`df.outbox.jobs{kind="pr.sync",result="gave_up"}`). A given-up "mark ready" comes back with the next completed run end or mark done of the task; a given-up "open" with the next push ahead of base or run end that pushed, as long as no PR is recorded.
- [During the rolling update, an old replica's `complete_task` writes only `tasks.completion_summary`] → That run's summary is not posted; the PR is still marked ready.
- [Job latency] → The dispatcher runs on the leader and is woken by `NOTIFY outbox`; the delay after a successful forge call is the notification round trip, not a poll interval.

## Migration Plan

One migration (D4). During a rolling update, old replicas still call the forge inline; a `pr.sync` job waits until a replica with the handler leads. Rollback: previous image; it reads `tasks.completion_summary`, which is kept, so summaries written by the new version are not posted by the old one.
