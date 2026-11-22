# Tasks

Product decisions are listed in design.md "Open Questions". The tasks follow the recommendations; a different decision changes the named tasks only.

## 1. Schema and configuration

- [ ] 1.1 Migration: `tasks.deleted_at`, `deleted_by`, `purged_at`, `closed_at`, `retention_hold` with checks (D1, D3, D8); `projects.task_retention_days` (`NULL`, `0` or `> 0`); `model_usage_totals` (D4); `transcript_blobs` with FKs to `tasks` and `blobs` (D6); partial index on `tasks (closed_at) WHERE deleted_at IS NULL`. Backfill `closed_at` and `transcript_blobs`. Verify with a `df-db` migration test on a database seeded with closed tasks and image events (backfilled rows match), and regenerate `.sqlx/`.
- [ ] 1.2 `df-core` config: `task_retention.delete_after_days` (`u32`, default `0` = off) and the effective-period rule (project override `NULL` / `0` / days). Unit tests for the rule; config parse test with and without the key.
- [ ] 1.3 Helm: `taskRetention.deleteAfterDays` (default `0`) in `values.yaml`, the config map and `README.md`; helm unit test in `tests/instance_config_test.yaml`.
- [ ] 1.4 Docs: Helm values reference and `docs/content/operations/` page "Task retention" (how to enable, the preview, space reuse by autovacuum, that deletion cannot be undone). Verify with `just docs-build`.

## 2. Closed time and tombstone (`df-db`)

- [ ] 2.1 Set and clear `closed_at` in every status update that enters or leaves `completed`, `cancelled`, `failed`, and in archive/unarchive. Tests: complete, cancel, fail, reopen, archive, unarchive.
- [ ] 2.2 `task_retention::delete_task(tx, key, principal, policy)`: lock the task row, check the preconditions of spec "Deletable tasks" (typed error naming the blocking run or subtask), write the tombstone (clear the fields of D1 on the task and its runs), audit `task.delete` (D10), publish the change notification. Tests for each precondition, the cleared and kept fields, and the audit details.
- [ ] 2.3 Reject on deleted tasks under the row lock: message queueing (board member, agent, child-ended — dropped with a log line), reopen, unarchive, rename, mark done, cancel, scheduler pick. One test per path that fails without the check.
- [ ] 2.4 `due_tasks(now)` query (effective period, `retention_hold`, deletable) and `set_retention_hold`, audited `task.retention_hold`. Tests: instance period, project override, project never, hold, open subtask, reopened task.

## 3. Purge (`df-db`, `df-orchestrator`)

- [ ] 3.1 `df-db` purge steps, each idempotent: transcript events in batches of 1,000; messages, artifacts, tool calls, agent requests, Git pushes, task-scoped egress rules; `model_usage` into `model_usage_totals` and delete (D4); blob references and unreferenced blobs (D6); `purged_at`. Tests per step, including a second run that changes nothing.
- [ ] 3.2 Write `transcript_blobs` in the same transaction as the blob put in `externalize_blob`, retry the put once on an FK violation. Test with two concurrent transactions (purge and writer of the same hash) in both commit orders: no event refers to a missing blob.
- [ ] 3.3 Coverage test: list every table with a `task_id` or `run_id` column from `information_schema` and fail for a table not classified as keep, delete or aggregate (design D2 risk).
- [ ] 3.4 `df_orchestrator::TaskRetention`: hourly leader loop with `every_period_while_leader`, tombstones due tasks (system principal, reason `retention`), then purges deleted tasks oldest first, volume through the existing provider (`volume.destroyed`, reason `retention`), stop checked between batches. Tests with `testing::Env`: due task deleted and purged, stop in the middle resumes, task with a running subtask skipped, late usage record of a purged run summed in the next scan.
- [ ] 3.5 Wire `TaskRetention` into the control plane next to `Retention` (`crates/dark-factory`), with shutdown handling. Verify the existing startup test passes and logs one line per purged task with counts.
- [ ] 3.6 Docs: `docs/content/architecture/agent-runs.md` (retention section: volume retention and task deletion, what stays and what is removed). Verify with `just docs-build`.

## 4. Usage and reads

- [ ] 4.1 Usage queries per run, task, persona and project read `model_usage` and `model_usage_totals`. Test: totals before and after a purge are equal.
- [ ] 4.2 Content endpoints of a deleted task answer `410 Gone` (transcript, live stream, messages, artifacts, artifact images). `df-web` tests per endpoint.
- [ ] 4.3 Factory tools: `read_artifact` and `list_artifacts` on a deleted task return "task <key> was deleted"; agent task listings carry a `deleted` flag. `df-agent-channel` tests. Docs: factory tools reference page under `crates/factory-agent/docs/` or `docs/content/using/delegation.md` (whichever lists the tools). Verify with `just docs-build`.

## 5. UI

- [ ] 5.1 Task page: Delete action with confirmation (disabled with the reason when not deletable), deleted task view with banner, runs and usage. Tests in `df-web` (`task_pages/tests`).
- [ ] 5.2 Task list: deleted tasks hidden from every filter, `?status=deleted` filter chip "Deleted" with deletion date. Tests for the default list, the filter and the live update on delete.
- [ ] 5.3 "deleted" marker on links to deleted tasks: parent, subtasks, dependencies, related tasks, memory provenance (when #302 is in). Tests per place.
- [ ] 5.4 Preview and Keep: due date on the task page within 7 days, Keep / remove exemption; project settings (General tab): retention period field (instance default / days / never) and the list of tasks due in the next 7 days. Tests for the due date computation and the form.
- [ ] 5.5 Chats: Delete in the archive modal, deleted chat page, deleted chats left out of the chat list and archive. Remove the "no delete action" test of `chats`. Tests in `df-web`.
- [ ] 5.6 Docs: `docs/content/using/tasks.md` (delete, deleted tasks, preview and Keep), `docs/content/using/chats.md` (delete an archived chat; remove "never deleted"), `docs/content/using/setup.md` (project retention setting). Verify with `just docs-build`.

## 6. Related changes

- [ ] 6.1 If `chat-emptydir-session-restore` is merged: delete `chat_session_states` in the purge; test. Otherwise add this step to that change's tasks.
- [ ] 6.2 If `iter6-agent-memory` is merged: delete `run_memory_context` and `transcript_messages` in the purge, filter deleted tasks in `search_history`, show "source deleted" in the memory provenance; tests. Otherwise add these steps to that change's tasks.
- [ ] 6.3 Integration check: an e2e scenario (`crates/df-e2e`) that deletes a seeded completed task and checks the deleted page, the hidden list entry and the parent link marker of its subtask.
