# Proposal

## Why

Nothing in Dark Factory is ever deleted. Transcripts (with their blobs), messages, artifacts, tool call records and usage records of every task stay in PostgreSQL forever, so the database only grows, and sensitive text pasted into a task cannot be removed. Board members need a way to delete old tasks by hand, and operators need an automatic retention period, without breaking the many references to a task (memories, subtasks, dependencies, audit entries, usage).

## What Changes

- **Tombstones:** deleting a task keeps its `tasks` row and its `runs` rows with the small identifying fields (key, kind, project, persona, status, title, branch, PR link, timestamps, run numbers and outcomes) and marks the task `deleted_at` / `deleted_by`. Every reference to the task or its runs stays valid, with no cascade logic.
- **Purge:** a background job in `df-orchestrator` removes the expensive parts of a deleted task in batches: transcript events and the blobs no other event uses, messages, artifacts, tool call records, agent requests, push records, task-scoped egress rules, the volume if it still exists, and (with the related changes) the chat session snapshot, the history-search rows and the per-run memory block. The task's description and long texts are cleared.
- **Usage totals:** before the per-request usage records of a run are deleted, they are summed into per-run, per-model totals. Usage views and sums per task, persona and project stay correct.
- **Manual delete:** board members delete a closed task (`completed`, `cancelled`, `failed`) or an archived chat from its page, after a confirmation. Never a task with a non-terminal run or a subtask that is not closed.
- **Automatic retention:** an instance-wide period (`taskRetention.deleteAfterDays`, **off by default**), overridable per project (another period or "never"). A task is due when it has been closed for the period. A task page shows "Will be deleted on …" from 7 days before; the project settings list what is due in the next 7 days; a board member can exempt a task ("Keep").
- **Subtrees:** each task follows its own schedule. A parent cannot be deleted while a child is open; closed children are not deleted with it.
- **Audit:** one audit entry `task.delete` per deleted task, naming the board member or the retention policy. The audit log itself is kept (no retention in this change).
- **UI:** deleted tasks are hidden from the task and chat lists by default (a "Deleted" filter shows them); a deleted task's page shows its header, runs and usage with "Deleted on … by …; transcript, messages and artifacts were removed"; links from memories, parents, children and dependencies stay and carry a "deleted" marker.
- **Agents:** a deleted task takes no messages and starts no runs; `read_artifact` / `list_artifacts` on it fail with "task deleted"; history search skips it.
- **BREAKING (spec):** the `chats` requirement "Chats are never deleted" is removed: an archived chat can be deleted like a closed task. Pre-MVP, no migration path needed.

## Capabilities

### New Capabilities

- `task-retention`: deleting tasks by hand and by retention period, the tombstone (what stays, what is removed), the purge job, the preview and the exemption, the audit entry, and how deleted tasks behave towards agents and other tasks.

### Modified Capabilities

- `chats`: "Chats are never deleted" is removed; archived chats follow `task-retention`.
- `task-ui`: deleted tasks in the task list, the deleted task page, the delete action, the deletion preview, the project retention settings and the "deleted" marker on links.
- `chat-ui`: Delete in the archive modal and the deleted chat page.
- `model-usage`: "Usage per run and model" also counts the stored totals of purged runs.
- `transcript-storage`: blobs are deleted when no stored transcript event refers to them.

## Impact

- `df-db`: migration (`tasks.deleted_at`, `deleted_by`, `purged_at`, `closed_at`, `retention_hold`; `projects.task_retention_days`; `model_usage_totals`; `transcript_blobs`), a `task_retention` module with the due query, tombstone and purge steps; the list, view and agent-read queries filter or mark deleted tasks.
- `df-orchestrator`: a purge scan next to `Retention` (same hourly leader loop), batch purge, volume deletion through the existing provider.
- `df-core`: config `task_retention.delete_after_days`; deletion preconditions.
- `df-web`: delete action and endpoint, deleted page, list filter, preview on the task page and in project settings, "Keep" toggle, project retention setting.
- `df-agent-channel`: artifact reads and history search reject or skip deleted tasks; message and run paths refuse deleted tasks.
- Helm: `taskRetention.deleteAfterDays` (default `0` = off).
- Docs: tasks, chats, project settings, Helm values, architecture (data retention).
- Related proposals: `chat-emptydir-session-restore` (DF-9, `chat_session_states`), `iter6-agent-memory` (PR #302, `memories`, `memory_changes`, `run_memory_context`, `transcript_messages`). Whichever lands second adds its tables to the purge.

## Out of Scope

- Retention of the audit log: backlog, row "Task retention and deletion" (listed there as a follow-up).
- A `task.deleted` lifecycle event for webhook subscribers: backlog, same row.
- Deleting a parent together with its closed subtree in one action: rejected (each task on its own schedule, see design D7).
- Undoing a deletion: rejected; the confirmation and the 7-day preview take its place.
- Hard delete of the `tasks` row (key reuse, GDPR-style erasure of titles): rejected for now; titles stay as the tombstone's identity.
- Retention of image build logs: stays in the backlog row of `project-image-builds`.
