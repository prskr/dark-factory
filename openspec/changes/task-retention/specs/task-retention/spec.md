# Spec Delta

## Purpose

Deletes old tasks and chats, by hand and after a retention period, so their transcripts, messages, artifacts and other large records leave the database, while a small tombstone keeps every reference to the task valid.

## ADDED Requirements

### Requirement: Deletable tasks
A task SHALL be deletable only when it is closed (`completed`, `cancelled` or `failed`; a chat: archived), has no non-terminal run, has no subtask that is not closed or deleted, and is not deleted yet. Any other delete request SHALL be rejected with the reason.

#### Scenario: Completed task
- **WHEN** a board member deletes `DF-1` in status `completed` without runs in progress
- **THEN** `DF-1` is deleted

#### Scenario: Open subtask
- **WHEN** a board member deletes completed `DF-19` while its subtask `DF-21` is `waiting`
- **THEN** the request is rejected with a reason naming `DF-21`

#### Scenario: Chat not archived
- **WHEN** a board member deletes `DF-12-CHAT` that is not archived
- **THEN** the request is rejected

#### Scenario: Run still finalizing
- **WHEN** a board member deletes cancelled `DF-3` while its last run is `finalizing`
- **THEN** the request is rejected

### Requirement: Manual delete
A board member SHALL be able to delete a deletable task. The deletion SHALL take effect at once: the task becomes a tombstone in the same request, and its content is no longer served.

#### Scenario: Delete from the task page
- **WHEN** a board member deletes `DF-1`
- **THEN** `DF-1` shows as deleted and its transcript, messages and artifacts are no longer returned by any endpoint

### Requirement: Tombstone
A deleted task SHALL keep its key, kind, project, persona, status, title, branch, base branch, PR number, PR URL, PR title, parent, base task, creator and timestamps, and SHALL record when and by whom (board member or retention policy) it was deleted. Its description, completion summary, status line and PR body SHALL be cleared.

#### Scenario: Deleted task keeps its identity
- **WHEN** `DF-1` with PR #42 and a 3 KB description is deleted
- **THEN** `DF-1` still has its title, status and PR #42, and its description is empty

### Requirement: Run tombstones
The runs of a deleted task SHALL keep their number, state, outcome and timestamps. Their detail text and completion summary SHALL be cleared.

#### Scenario: Run kept
- **WHEN** `DF-1` with runs 1 and 2 is deleted
- **THEN** both runs still exist with their numbers and outcomes, and memory provenance naming run 2 still resolves

### Requirement: Purge of task content
Within the next hourly scan after the deletion, the control plane SHALL remove the deleted task's transcript events, messages, artifacts, tool call records, agent requests and their push-check exceptions, Git push records, per-request usage records, task-scoped egress rules, chat session snapshot, history-search rows and per-run memory blocks, and SHALL delete its volume if one exists. It SHALL then mark the task as purged.

#### Scenario: Purge after delete
- **WHEN** `DF-1` with 40,000 transcript events and 3 artifacts is deleted
- **THEN** after the next scan no transcript event, message or artifact of `DF-1` is stored and `DF-1` is marked purged

#### Scenario: Volume still present
- **WHEN** `DF-2` is deleted 2 days after it was cancelled and its volume still exists
- **THEN** the scan deletes the volume

### Requirement: Purge in batches
The purge SHALL remove content in bounded batches, each in its own transaction, and SHALL resume where it stopped after a restart or a leader change.

#### Scenario: Leader change during a purge
- **WHEN** the leader stops while `DF-1` is half purged
- **THEN** the next leader's scan purges the rest of `DF-1` and marks it purged

### Requirement: References to deleted tasks stay valid
Memories and their history, parent and base task references, dependencies, audit entries, lifecycle events and usage totals that refer to a deleted task or its runs SHALL stay unchanged.

#### Scenario: Memory from a deleted task
- **WHEN** a memory was created by run 2 of `DF-1` and `DF-1` is deleted
- **THEN** the memory stays and its provenance still names `DF-1` run 2

#### Scenario: Dependency on a deleted task
- **WHEN** `DF-24` depends on completed `DF-23` and `DF-23` is deleted
- **THEN** the dependency of `DF-24` stays satisfied

### Requirement: Deleted tasks are final
A deleted task SHALL take no messages, SHALL NOT start runs, and SHALL NOT be reopened, unarchived, renamed or marked done. A deletion SHALL NOT be undone.

#### Scenario: Message to a deleted task
- **WHEN** a board member or an agent queues a message on deleted `DF-1`
- **THEN** the request is rejected and nothing is queued

### Requirement: Agent reads of deleted tasks
Factory tools that read another task's content SHALL fail for a deleted task with an error that says the task was deleted, and history search SHALL NOT return hits from deleted tasks.

#### Scenario: Read an artifact of a deleted task
- **WHEN** an agent calls `read_artifact` for related task `DF-1`, which was deleted
- **THEN** the tool returns an error saying `DF-1` was deleted

### Requirement: Retention period
The instance SHALL have a task retention period in days, off by default. A project SHALL be able to override it with another period or with "never". A deletable task SHALL be due when it has been closed (or archived) for at least the effective period and is not exempt.

#### Scenario: Off by default
- **WHEN** an instance runs with the default configuration
- **THEN** no task is deleted automatically

#### Scenario: Project override
- **WHEN** the instance period is 180 days and project `DF` sets 30 days
- **THEN** a task of `DF` closed 31 days ago is due and a task of another project closed 31 days ago is not

#### Scenario: Project never
- **WHEN** the instance period is 180 days and project `OPS` sets "never"
- **THEN** no task of `OPS` is deleted automatically

### Requirement: Closed time
The closed time of a task SHALL be set when it enters `completed`, `cancelled` or `failed` (a chat: when archived) and SHALL be cleared when it leaves that state. Retention SHALL count from the closed time.

#### Scenario: Reopened task
- **WHEN** `DF-3` was cancelled 200 days ago, reopened, and cancelled again 10 days ago, with a period of 180 days
- **THEN** `DF-3` is not due

### Requirement: Automatic deletion
The leader's hourly scan SHALL delete every due task as the retention policy, in the same way as a manual delete.

#### Scenario: Due task
- **WHEN** the period is 90 days and `DF-1` has been `completed` for 91 days
- **THEN** `DF-1` is deleted within the next hour with the retention policy as deleter

#### Scenario: Not deletable when due
- **WHEN** completed `DF-19` is due but its subtask `DF-21` is running
- **THEN** `DF-19` is skipped and checked again in the next scan

### Requirement: Deletion preview
From 7 days before its due date, a task SHALL show the date it will be deleted, and the project settings SHALL list the tasks due in the next 7 days.

#### Scenario: Due in five days
- **WHEN** `DF-1` will be due in 5 days
- **THEN** its page says "Will be deleted on <date>" and the project's retention list contains `DF-1`

### Requirement: Keep a task
A board member SHALL be able to exempt a task from automatic deletion and remove the exemption again. An exempt task SHALL still be deletable by hand.

#### Scenario: Kept task
- **WHEN** a board member marks `DF-1` "Keep" and its due date passes
- **THEN** `DF-1` is not deleted automatically

### Requirement: Subtasks keep their own schedule
Deleting a task SHALL NOT delete its subtasks, and a subtask SHALL be due on its own closed time.

#### Scenario: Parent deleted first
- **WHEN** `DF-19` and its subtask `DF-21` are completed and `DF-19` is deleted
- **THEN** `DF-21` stays and its parent link shows `DF-19` as deleted

### Requirement: Deletion audit
Each deletion SHALL write an audit entry `task.delete` with the task key, kind and project, and as actor the board member or the retention policy with the effective period.

#### Scenario: Automatic deletion audited
- **WHEN** the retention scan deletes `DF-1` under a 90-day period
- **THEN** an audit entry `task.delete` for `DF-1` names the retention policy and 90 days

#### Scenario: Keep audited
- **WHEN** a board member marks `DF-1` "Keep"
- **THEN** an audit entry `task.retention_hold` names the board member and `DF-1`
