# Design

## Context

- Every table with task or run data references `tasks` or `runs` with `ON DELETE CASCADE` (`runs`, `task_messages`, `transcript_events`, `tool_calls`, `agent_requests`, `push_check_exceptions`, `git_pushes`, `model_usage`, `task_artifacts`, task-scoped `egress_allowlist` rows). `task_dependencies`, `tasks.parent_id` and `tasks.base_task_id` reference `tasks` without cascade, so a `DELETE FROM tasks` fails as soon as a task has a child or a dependant.
- `blobs` is content-addressed and shared: transcript payloads refer to it by SHA-256 inside JSON (`transcript/runtime.rs`, `externalize_blob`). Nothing records which event uses which blob, and nothing deletes blobs.
- `audit_log`, `lifecycle_events` and `outbox` carry task ids or keys as text or JSON, without a foreign key. `lifecycle_events` and `outbox` already have their own cleanup.
- The egress log is an in-memory ring buffer in the gateway (max. 1 hour); nothing is persisted per flow.
- `df_orchestrator::Retention` scans hourly on the leader and deletes task volumes (`volumes.retainAfterCloseSeconds`, `volumes.idleTtlSeconds`), marking `tasks.volume_deleted_at`. Its loop helper `every_period_while_leader` and the stop handling are reusable.
- Related proposals not merged yet: `chat-emptydir-session-restore` (DF-9) adds `chat_session_states` (one snapshot per chat, `ON DELETE CASCADE`). `iter6-agent-memory` (PR #302) adds `memories` and `memory_changes` (provenance `created_by_task_id`/`run_id`, `ON DELETE SET NULL`), `run_memory_context` (per-run block, cascade) and `transcript_messages` (history search, cascade), and a `tasks.search` column.

## Goals / Non-Goals

**Goals:**

- Remove the large and sensitive data of old tasks, by hand and by policy.
- Keep every reference to a task or a run valid without new cascade rules.
- Make automatic deletion predictable: off unless configured, visible a week ahead, exemptible, audited.

**Non-Goals:**

- Audit log retention: backlog, row "Task retention and deletion" (follow-ups listed there).
- A `task.deleted` lifecycle event: backlog, same row.
- Undo or restore of a deleted task: rejected.
- Hard delete of `tasks` rows: rejected (D1).
- Physical space reclaim (`VACUUM FULL`): rejected; autovacuum reuses the space, the docs say so.

## Decisions

### D1 — Tombstones for tasks and runs

`tasks` and `runs` rows stay. New columns on `tasks`: `deleted_at TIMESTAMPTZ`, `deleted_by JSONB` (a `Principal`: user, or system with reason `retention`), `purged_at TIMESTAMPTZ`. Check: `deleted_at` and `deleted_by` are both null or both set; `purged_at` only with `deleted_at`.

The tombstone step (one transaction) clears on `tasks`: `description`, `completion_summary`, `status_line`, `pr_body`, `harness_session_id`; on `runs`: `detail`, `completion_summary`, `harness_session_id`, `pod_ip`. It keeps key, kind, number, project, persona, status, title, title source, branch, base branch, PR number/URL/title, parent, base task, creator, archive record and all timestamps; on runs number, state, outcome, finalize reason, SHAs and timestamps.

Alternative: delete `runs` and keep only `tasks`. Rejected: memory provenance and `memory_changes.run_id` are `ON DELETE SET NULL`, so "undo this run" and the author line would lose the run; usage totals would need a new key.

### D2 — Keep, delete or aggregate, per table

| Table / data | Action | Note |
| --- | --- | --- |
| `tasks` | keep (tombstone) | long texts cleared (D1) |
| `runs` | keep (tombstone) | detail texts cleared (D1) |
| `transcript_events` | delete | in batches (D5) |
| `blobs` | delete if unreferenced | via `transcript_blobs` (D6) |
| `task_messages` | delete | includes `forge_event` bytes |
| `task_artifacts` | delete | content is in the row |
| `tool_calls` | delete | arguments and results |
| `agent_requests` | delete | decisions are in the audit log |
| `push_check_exceptions` | delete | cascade with `agent_requests` |
| `git_pushes` | delete | per-push details; pushes are in the forge |
| `model_usage` | aggregate, then delete | into `model_usage_totals` (D4) |
| `egress_allowlist` (task scope) | delete | rules of a closed task are dead |
| `task_dependencies` | keep | status of the deleted task is kept, so satisfaction is unchanged |
| `audit_log` | keep | no FK; resource id is the key |
| `lifecycle_events`, `outbox` | keep | own cleanup; handlers already tolerate missing data |
| task volume (PVC) | delete | if `volume_deleted_at` is null, via the existing provider; event `volume.destroyed` reason `retention` |
| egress log | nothing | in memory only, max. 1 hour |
| `chat_session_states` (DF-9) | delete | snapshot of the chat |
| `memories`, `memory_changes` (#302) | keep | provenance stays valid; UI marks the source deleted |
| `run_memory_context` (#302) | delete | per-run copy of memory text |
| `transcript_messages` (#302) | delete | history search text |
| `tasks.search` (#302) | regenerated | from title only, description is empty; history search filters deleted tasks anyway |

Whichever of DF-9, #302 and this change lands last adds the rows of the others to the purge (tasks 6.1, 6.2).

### D3 — Which tasks, and when

Deletable (spec "Deletable tasks"): status `completed`, `cancelled` or `failed` (chat: archived), no non-terminal run, every subtask closed or deleted, not deleted yet. The check runs inside the tombstone transaction with the task row locked `FOR UPDATE`, so a concurrent message, reopen or unarchive either commits first (and the check fails) or sees `deleted_at` and is rejected.

New column `tasks.closed_at`: set in the same update that puts a task into a closed status or archives a chat; cleared when it leaves. Migration backfills it from the later of `updated_at` and the last run's `ended_at` (chats: `archived_at`). Retention counts from `closed_at`, not from `updated_at`, which a rename changes.

Effective period: `projects.task_retention_days` (`NULL` = instance default, `0` = never, `> 0` = days), else `taskRetention.deleteAfterDays` (instance config, `0` = off, default `0`). Due: deletable, `retention_hold = false`, `closed_at <= now() - period`.

### D4 — Usage totals

New table `model_usage_totals (run_id FK runs, model TEXT NOT NULL DEFAULT '', wire_api, requests, unknown_requests, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens)`, PK `(run_id, model, wire_api)`. The purge sums a run's `model_usage` rows into it and deletes them in the same transaction. The usage queries (per run, task, persona, project) read the union of both tables. Records that arrive for a purged run (late gateway report) go to `model_usage` and are summed by the next scan: the purge of `model_usage` runs on every scan for purged tasks that got new rows, a cheap indexed query.

Alternative: keep `model_usage` (about 100 bytes per request). Rejected: busy tasks have thousands of requests, and the per-request time and status add nothing after deletion.

### D5 — Purge job and batches

One loop in `df-orchestrator`, `TaskRetention`, next to `Retention`, with the same `every_period_while_leader` helper and stop signal, hourly. Each scan:

1. If retention is on anywhere (instance or a project period), select due tasks (indexed on `closed_at` where `deleted_at IS NULL`) and tombstone each in its own transaction, with audit entry.
2. Select tasks with `deleted_at` set and `purged_at` null, oldest first, and purge each: delete transcript events of the task in batches of 1,000 rows per transaction (`DELETE … WHERE id IN (SELECT id … LIMIT 1000)`), then the other tables of D2 (small, one transaction each), then the volume, then the blobs (D6), then set `purged_at`.

Every step is idempotent, so a stop or leader change resumes at the next scan. The stop signal is checked between batches.

Manual delete runs step 1 for one task in the HTTP request. Content is hidden at once because every content query filters `deleted_at`; the bytes leave at the next scan (at most an hour). Alternative: purge in the request. Rejected: a large transcript would hold the request and a long transaction.

### D6 — Blob references

New table `transcript_blobs (task_id FK tasks, sha256 FK blobs, PRIMARY KEY (task_id, sha256))`. `externalize_blob` stores the blob and the reference in one transaction. The purge, after all events of the task are gone, deletes the task's references, returns their hashes, and deletes each blob with no other reference (`DELETE FROM blobs WHERE sha256 = $1 AND NOT EXISTS (…)`).

Races: the foreign key from `transcript_blobs` to `blobs` makes both orders safe. If a writer adds a reference while the purge deletes the blob, either the writer's reference commits first and the delete fails the FK check (the purge skips that blob), or the delete commits first and the writer's insert fails the FK check (the writer retries the put once, which re-creates the blob). The migration backfills `transcript_blobs` with `jsonb_path_query(payload, 'strict $.**.sha256')` over existing events.

Alternative: a periodic mark-and-sweep over all payloads. Rejected: a full scan of every transcript payload per run of the sweep.

### D7 — Subtasks: each on its own schedule

A parent can be deleted when its children are closed or deleted; it does not delete them. Each child is due on its own `closed_at`. The tombstone keeps `parent_id`, so the child shows the parent as deleted. A child that is reopened later and ends sends its child-ended message to a deleted parent: the message is dropped with a log line (spec "Deleted tasks are final").

Alternative: delete the subtree with the parent. Rejected: one action with many effects, extra confirmation logic, and children may be referenced from elsewhere.

### D8 — Preview and Keep

`tasks.retention_hold BOOLEAN NOT NULL DEFAULT false`. The task page computes the due date from `closed_at` and the effective period and shows it when at most 7 days away (constant). The project settings list tasks due in the next 7 days. Keep toggles `retention_hold`, audited as `task.retention_hold` with the new value. Manual delete ignores the hold.

### D9 — Endpoints and agent paths

- `POST /tasks/{key}/delete` (board members; chats use the same endpoint), `POST /tasks/{key}/retention-hold` (`on` / `off`), project setting in the existing project settings form (General tab).
- Every content endpoint of a deleted task (transcript, live stream, messages, artifacts, artifact images) answers `410 Gone`. Task page and chat page render the deleted view.
- Message queueing, reopen, unarchive, rename, mark done and run scheduling check `deleted_at` under the task row lock.
- Factory tools: `read_artifact` and `list_artifacts` on a deleted related task return the error "task <key> was deleted"; `search_history` (#302) filters `deleted_at IS NULL`; task listings for agents keep deleted tasks with a `deleted` flag.
- A change notification is published on delete, so lists update live.

### D10 — Audit

`task.delete`, resource type `task`, resource id the key, details: kind, project key, `policy` (`manual` or `retention`) and, for retention, `period_days` and its source (`instance` or `project`). The actor is the board member or the system principal with reason `retention`. One entry per task, written in the tombstone transaction. Changes of the project period are audited with the existing project settings audit (before and after). The purge itself writes no audit entry (one log line per task with counts).

## Risks / Trade-offs

- [A wrong period deletes much at once] → Off by default; the project list shows what is due within 7 days; Keep; audit per task; tombstones keep the task's identity. The first scan after enabling only deletes what is already due, so the docs tell operators to check the preview first.
- [Sensitive text in a title stays] → Board members rename the task before deleting it. Hard delete is rejected (D1).
- [Large purges load the database] → Batches of 1,000 events, one task at a time, hourly. Space is reused by autovacuum, not returned to the OS (docs).
- [A blob is lost while another run stores it] → Foreign key on `transcript_blobs` (D6), test with two concurrent transactions.
- [DF-9 or #302 land after this change and add a table that the purge misses] → The purge has a test that lists every table with a `task_id` or `run_id` column from `information_schema` and fails for a table not classified in D2.
- [Backfill of `transcript_blobs` on large instances is slow] → Pre-MVP, small databases; the migration runs once.

## Migration Plan

One migration: new columns and tables, backfill of `closed_at` and `transcript_blobs`. No data is deleted until an operator sets `taskRetention.deleteAfterDays` or a board member deletes a task. Rollback: the columns are additive; a rollback before any deletion loses nothing. After a deletion, the content is gone (no restore).

## Open Questions

Product decisions, each with the recommendation the specs follow:

1. **Description: keep or clear?** Clear it, together with completion summary, status line and PR body. They can be long and contain sensitive text; title and PR link identify the task.
2. **Automatic retention default: off or generous (for example 180 days)?** Off. Deletion cannot be undone, and an upgrade must not start deleting. Operators turn it on; the docs suggest 180 days.
3. **Audit log: own retention or kept?** Kept in this change. It is small, has no task content, and is the record of the deletions. Own retention later (backlog).
4. **Subtrees: delete together or each on its own schedule?** Each on its own (D7).
5. **Is `failed` deletable?** Yes. It is in the "Done" group, and a failed task with no new message for the period is abandoned.
6. **Grace: preview window and Keep?** Preview of 7 days on the task page and in project settings, plus Keep (D8). No soft-delete period with restore.
7. **Deleted tasks in lists?** Hidden by default, a "Deleted" filter shows them; the deleted chat drops out of the archive.
8. **Who may delete by hand?** Every board member, like cancel and archive. Alternative: admins only.
9. **Runs: tombstone or delete?** Tombstone (D1), needed by memory provenance and usage totals.
10. **`agent_requests` and `git_pushes`: delete or keep?** Delete; the decisions and pushes are in the audit log and the forge.
11. **Lifecycle event `task.deleted`?** Not in this change (backlog); subscribers learn about the end of a task from the existing events.
12. **Purge delay after a manual delete?** Up to one hour (next scan); content is hidden at once. Alternative: wake the scan by notification for faster removal.
