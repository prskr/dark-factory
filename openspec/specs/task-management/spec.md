# task-management Specification

## Purpose

Defines tasks as the unit of delegated work: how they are identified, how creating a task and sending a message turn into pending work for an agent, and the status vocabulary every component shares.

## Requirements

### Requirement: Per-project task keys
Every task SHALL get a key `<PROJECT_KEY>-<n>` where `n` starts at 1 and increases by one per project without gaps or reuse, also under concurrent creation. Project keys SHALL match `^[A-Z][A-Z0-9]{1,9}$`.

#### Scenario: First task of a project
- **WHEN** the first task is created in project `DF`
- **THEN** its key is `DF-1`

#### Scenario: Concurrent creation
- **WHEN** two tasks are created in project `DF` at the same time
- **THEN** they receive two distinct consecutive keys

#### Scenario: Invalid project key
- **WHEN** a project is saved with key `df` or `D`
- **THEN** it is rejected with a validation error

### Requirement: Derived task names
The task branch SHALL be `factory/task/<KEY>`, the WIP snapshot branch `factory/wip/<KEY>` and the task volume name `task-<key in lower case>`.

#### Scenario: Names for DF-1
- **WHEN** the names for task `DF-1` are derived
- **THEN** they are `factory/task/DF-1`, `factory/wip/DF-1` and `task-df-1`

### Requirement: Task creation queues the assignment prompt
Creating a task SHALL store description, project, assigned persona, optional linked issue number, creator (a principal), optional parent, dependencies and an optional title, and SHALL queue, in the same transaction, one prompt from the system principal (reason `task.assigned`) with the description. Without a title from its creator, the prompt SHALL ask the persona to call `set_task_title` in a separate `hidden` content block; the description SHALL be visible.

#### Scenario: Task created
- **WHEN** a board member creates a task with description "Fix login" assigned to persona "Linus"
- **THEN** the task exists with status `queued` and one undelivered prompt with author reason `task.assigned`

#### Scenario: Prompt asks for a title
- **WHEN** a task is created without a title
- **THEN** its assignment prompt contains the description in a visible block and, in a block with `hidden` set, the request to call `set_task_title`

#### Scenario: Title given by a person
- **WHEN** a task is created with a title
- **THEN** the assignment prompt contains no hidden block

#### Scenario: Task created by an agent
- **WHEN** the agent of `DF-20` creates a task with a title and a prompt
- **THEN** the task's creator is the agent of `DF-20`, its parent is `DF-20`, and its assignment prompt contains the prompt and no hidden block

### Requirement: Queued messages
A human SHALL be able to queue a message on any task at any time. Each message SHALL get a unique prompt id, its author and queue time, and stays undelivered until a transcript `UserMessage` event with that prompt id is persisted. If the task has an active run that is not `finalizing`, the status SHALL become `running`; if the task has never started a run and has an unsatisfied dependency, it SHALL become or stay `awaiting_dependencies`; otherwise it SHALL become `queued`.

#### Scenario: Message on a waiting task
- **WHEN** a board member queues "also add a test for X" on a task in status `waiting` without an active run
- **THEN** an undelivered prompt is stored and the task status becomes `queued`

#### Scenario: Message on a failed task
- **WHEN** a message is queued on a task in status `failed`
- **THEN** the task status becomes `queued`

#### Scenario: Message on a task with an idle run
- **WHEN** a message is queued on a `waiting` task whose run is `awaiting_input`
- **THEN** the task status becomes `running` and no new run is scheduled

#### Scenario: Message on a task whose run is finalizing
- **WHEN** a message is queued on a task whose run is `finalizing`
- **THEN** the task status becomes `queued` and the next run starts once that run has ended

#### Scenario: Message on a task in review
- **WHEN** a message is queued on a task in status `in_review`
- **THEN** the task status becomes `queued` and a new run starts

#### Scenario: Message on a task awaiting dependencies
- **WHEN** a message is queued on a task in status `awaiting_dependencies`
- **THEN** the task stays `awaiting_dependencies` and no run starts

#### Scenario: Reopening a cancelled task that never started
- **WHEN** a message is queued on a cancelled task that never ran and still has an unsatisfied dependency
- **THEN** the task becomes `awaiting_dependencies`

### Requirement: Task status vocabulary
A task SHALL have exactly one status: `awaiting_dependencies` (unsatisfied dependencies, never started), `queued` (pending work, no active run), `running` (a turn runs or is about to start), `waiting` (last turn ended, waits for a human; the run may be kept alive), `blocked` (no answer or approval within the idle deadline), `in_review` (`complete_task` called; the PR waits for review), `completed` (marked done or PR merged), `cancelled`, `rate_limited`, `limit_reached` or `failed`.

#### Scenario: Status is always one of the defined values
- **WHEN** any component writes a task status outside this set
- **THEN** the database rejects the write

#### Scenario: Message on a blocked task
- **WHEN** a message is queued on a task in status `blocked`
- **THEN** the task status becomes `queued`

#### Scenario: New task with an unsatisfied dependency
- **WHEN** a task is created with a dependency that is not satisfied
- **THEN** its status is `awaiting_dependencies`

### Requirement: Task status groups
`awaiting_dependencies`, `waiting`, `blocked`, `in_review` and `limit_reached` SHALL form the "Waiting" group of the task list filters, `completed` and `failed` the "Done" group.

#### Scenario: Waiting group
- **WHEN** the task list is filtered by the "Waiting" group
- **THEN** it lists the tasks in status `awaiting_dependencies`, `waiting`, `blocked`, `in_review` and `limit_reached`

### Requirement: Change notifications
Writes that change a task, run, transcript or queued message SHALL publish a notification after commit, so any control plane replica can react without polling.

#### Scenario: Message queued on another replica
- **WHEN** replica A queues a message for task `DF-1`
- **THEN** replica B receives a `task_message_queued` notification naming the task

### Requirement: Task title sources
A task's title SHALL come from one of four sources, ranked user > generated > agent > placeholder. A task created without a title SHALL get the first sentence of its description, cut to 80 characters, as placeholder. A source SHALL only replace a title from the same or a lower-ranked source, except that agent and generated titles are set at most once.

#### Scenario: Placeholder at creation
- **WHEN** a task is created with description "The nightly backup job sometimes runs twice. It started last week."
- **THEN** its title is "The nightly backup job sometimes runs twice." with source `placeholder`

#### Scenario: Agent title after a rename
- **WHEN** a board member renamed the task and the persona then sets a title
- **THEN** the title stays the board member's

### Requirement: Titles are sanitized
Every title from an agent or a generator SHALL be stored as plain text: control characters and line breaks replaced by spaces, whitespace collapsed, trimmed and cut to 80 characters. An empty result SHALL be rejected and leave the title unchanged.

#### Scenario: Title with a line break
- **WHEN** the persona sets the title "Fix login\n\nand logout"
- **THEN** the stored title is "Fix login and logout"

### Requirement: Rename
A board member SHALL be able to rename a task to a non-empty title of at most 80 characters. A rename SHALL set the source to `user`, write an audit entry `task.rename` with the old and new title, and publish a `task_changed` notification.

#### Scenario: Rename DF-15
- **WHEN** a board member renames DF-15 to "Stop duplicate nightly backups"
- **THEN** the title is stored with source `user` and an audit entry `task.rename` exists

### Requirement: Queued messages reach the agent between turns
An undelivered message SHALL be forwarded to the task's active run only while the run is between turns: when the run starts, and after each turn ends. Forwarding SHALL mark the message as forwarded. A message queued during a turn SHALL stay held back until that turn ends.

#### Scenario: Message during a turn
- **WHEN** a board member queues a message while the agent is in a turn
- **THEN** the message is not sent to the runtime until the transcript records the end of that turn

#### Scenario: Message on an idle run
- **WHEN** a message is queued while the active run is between turns
- **THEN** the message is forwarded to the runtime and marked as forwarded

#### Scenario: Reconnect after forwarding
- **WHEN** the runtime reconnects while a forwarded message is still undelivered
- **THEN** the message is sent again with the same prompt id and is delivered once

### Requirement: Editing and removing queued messages
A board member SHALL be able to replace the text of, or remove, a queued message written by a human as long as it is neither forwarded nor delivered. Both SHALL be refused once the message is forwarded; prompts authored by the system (such as the assignment prompt) SHALL NOT be editable or removable. Both SHALL write an audit entry (`message.edit` with old and new text, `message.remove` with the removed text) and publish a change notification.

#### Scenario: Edit before forwarding
- **WHEN** a board member edits a held-back message to "also check logout"
- **THEN** the stored message has the new text, keeps its prompt id and queue position, and an audit entry `message.edit` exists

#### Scenario: Edit after forwarding
- **WHEN** a board member edits a message that was forwarded in the meantime
- **THEN** the edit is refused with "The agent has already received this message." and the message is unchanged

#### Scenario: Remove before forwarding
- **WHEN** a board member removes a held-back message
- **THEN** the message no longer exists and an audit entry `message.remove` exists

#### Scenario: Assignment prompt
- **WHEN** a board member tries to remove the assignment prompt of a new task
- **THEN** the removal is refused and the prompt stays queued

#### Scenario: Empty edit
- **WHEN** a queued message is edited to an empty or whitespace-only text
- **THEN** the edit is refused and the message is unchanged

### Requirement: Removing the last queued message
When a removal leaves a task in status `queued` with no undelivered message, the task SHALL return to `waiting`, so no run starts for nothing.

#### Scenario: Last message of a waiting task removed
- **WHEN** a board member queues a message on a `waiting` task and removes it before a run starts
- **THEN** the task status is `waiting` again

#### Scenario: Other messages still queued
- **WHEN** two messages are queued on a `waiting` task and one is removed
- **THEN** the task stays `queued`

### Requirement: Mark a task done
A board member SHALL be able to mark a task done unless it is `queued`, `running` or already `completed`. The task SHALL become `completed`; if it has a run in `awaiting_input`, the control plane SHALL cancel that run. The change SHALL be audited with the actor. Undelivered messages stay queued; a new message reopens the task as for any `completed` task.

#### Scenario: Close a waiting task
- **WHEN** a board member marks `DF-1` in status `waiting` done
- **THEN** `DF-1` becomes `completed` and an audit entry names the board member

#### Scenario: Not while working
- **WHEN** a board member tries to mark a `running` task done
- **THEN** the request is rejected and the status does not change

#### Scenario: Not twice
- **WHEN** a board member tries to mark a `completed` task done
- **THEN** the request is rejected as a conflict and no audit entry is written

### Requirement: Marking done readies the PR
If a task marked done has a PR that isn't attached, the control plane SHALL mark it ready for review after the status change, without a summary. A forge failure SHALL be logged and SHALL NOT undo the status.

#### Scenario: Close a waiting task with a PR
- **WHEN** a board member marks `DF-1` in status `waiting` with a draft PR done
- **THEN** `DF-1` becomes `completed`, the PR is no longer a draft and gets no new comment

#### Scenario: Mark done without a PR
- **WHEN** a board member marks a task without a PR done
- **THEN** the task becomes `completed` and the forge is not called

#### Scenario: Mark done with an attached PR
- **WHEN** a board member marks `DF-15`, attached to a human's draft PR #45, done
- **THEN** `DF-15` becomes `completed` and the forge is not called

### Requirement: Cancelling a task
A board member SHALL be able to cancel a task in any status except `completed` and `cancelled` (including `in_review`). Cancelling SHALL set the status `cancelled`, stop its active run, mark its undelivered messages as discarded and write an audit entry `task.cancel`. The task's PR and linked issue SHALL NOT be changed.

#### Scenario: Cancel a running task
- **WHEN** a board member cancels `DF-9` while its run is active and one message is undelivered
- **THEN** the run is stopped, the message is discarded, `DF-9` is `cancelled` and its draft PR is still open

#### Scenario: Cancel a completed task
- **WHEN** a board member tries to cancel a completed task
- **THEN** the action is rejected and nothing changes

### Requirement: Reopening a cancelled task
Queueing a message on a cancelled task SHALL set it to `queued`. Discarded messages SHALL NOT be delivered.

#### Scenario: Reopen
- **WHEN** a board member queues "please continue after all" on cancelled task `DF-9`
- **THEN** `DF-9` becomes `queued` and the next run receives only the new message

#### Scenario: Reopen before the stopped run ended
- **WHEN** a message is queued on cancelled task `DF-9` while its stopped run is still finalizing
- **THEN** `DF-9` stays `queued` when that run's `RunReport` arrives, and a new run starts with the message after the old run ended

### Requirement: Resume time and count
A task SHALL carry an optional resume time (when the scheduler may start its next run) and a resume count (consecutive automatic resumes after infrastructure failures). Queueing a human message SHALL clear the resume time and set the count to 0.

#### Scenario: Message clears the resume time
- **WHEN** a board member queues a message on `DF-2`, which is `rate_limited` with a resume time 40 minutes ahead
- **THEN** `DF-2` becomes `queued` with no resume time and resume count 0, and its next run starts within 5 seconds

### Requirement: Volume retention
Once per hour the control plane SHALL delete the volumes of tasks without a non-terminal run that are `completed` or `cancelled` with their last run older than `volumes.retainAfterCloseSeconds` (default 7 days), or whose last run is older than `volumes.idleTtlSeconds` (default 30 days), and SHALL remember the deletion so it is not repeated.

#### Scenario: Closed task after the grace period
- **WHEN** `DF-1` has been `completed` for 8 days
- **THEN** its volume is deleted within the next hour

#### Scenario: Idle task
- **WHEN** `DF-2` is `waiting` and its last run ended 31 days ago
- **THEN** its volume is deleted and queueing a message later starts a run on a fresh volume

#### Scenario: Active run is skipped
- **WHEN** a task matches the idle TTL but has a run in `running`
- **THEN** its volume is kept

### Requirement: Reset workspace
A board member SHALL be able to delete the volume of a task without a non-terminal run. The action SHALL write an audit entry `task.reset_workspace` and SHALL be rejected while a run is active.

#### Scenario: Reset a waiting task
- **WHEN** a board member resets the workspace of `DF-3` in status `waiting`
- **THEN** its PVC is deleted, `task.reset_workspace` is audited and the next run clones afresh and restores the WIP snapshot if one exists

#### Scenario: Reset rejected while running
- **WHEN** a board member resets the workspace of a task with a run in `preparing`
- **THEN** the action is rejected and the volume is kept

### Requirement: Merged PR completes the task
When the forge reports the task's PR as merged, the task SHALL become `completed` unless it is `completed` or `cancelled`, SHALL remember the merge time, and SHALL stop its active run. An audit entry `task.done` SHALL name the system principal with reason `pr.merged` and the merging login. Undelivered messages stay queued, as for a task marked done by hand.

#### Scenario: Merged while in review
- **WHEN** the PR of `DF-1` in status `in_review` is merged by `alice`
- **THEN** `DF-1` becomes `completed` and the audit entry names reason `pr.merged` and `alice`

#### Scenario: Merged while running
- **WHEN** the PR of `DF-2` is merged while its run is in a turn
- **THEN** the run is stopped and `DF-2` is `completed`, not `cancelled`

#### Scenario: Merged after cancel
- **WHEN** the PR of cancelled task `DF-3` is merged
- **THEN** `DF-3` stays `cancelled`, and the merge time is remembered

#### Scenario: Redelivered merge event
- **WHEN** the merge event of `DF-1` arrives twice
- **THEN** `DF-1` is `completed` once and one audit entry exists

### Requirement: Volume of a merged task
The retention scan SHALL delete the volume of a task without a non-terminal run whose PR was merged, without waiting for `volumes.retainAfterClose`.

#### Scenario: Merged an hour ago
- **WHEN** the PR of `DF-1` was merged and its stopped run has ended
- **THEN** the next retention scan deletes the volume of `DF-1`

### Requirement: Reopening needs the branch
A message that would move a task in status `completed`, `failed` or `cancelled` back to `queued` SHALL be rejected with a conflict naming the other task if that task now holds the same branch. Nothing SHALL be queued.

#### Scenario: Branch taken over
- **WHEN** `DF-13` on `renovate/serde-1.x` was cancelled, `DF-16` was created on `renovate/serde-1.x`, and a board member queues a message on `DF-13`
- **THEN** the message is rejected with a conflict naming `DF-16` and `DF-13` stays `cancelled`

#### Scenario: Branch free
- **WHEN** a board member queues a message on cancelled `DF-13` while no other open task holds `renovate/serde-1.x`
- **THEN** `DF-13` becomes `queued`
