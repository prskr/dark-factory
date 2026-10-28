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
Creating a task SHALL store description, project, assigned persona, optional linked issue number, creator and an optional title, and SHALL queue one prompt authored by the system principal (reason `task.assigned`) containing the description, in the same transaction. When the task has no title from the user, the prompt SHALL ask the persona to set one with `set_task_title`, in a separate content block marked `hidden`; the description SHALL be in visible blocks.

#### Scenario: Task created
- **WHEN** a board member creates a task with description "Fix login" assigned to persona "Linus"
- **THEN** the task exists with status `queued` and one undelivered prompt with author reason `task.assigned`

#### Scenario: Prompt asks for a title
- **WHEN** a task is created without a title
- **THEN** its assignment prompt contains the description in a visible block and, in a block with `hidden` set, the request to call `set_task_title`

#### Scenario: Title given by a person
- **WHEN** a task is created with a title
- **THEN** the assignment prompt contains no hidden block

### Requirement: Queued messages
A human SHALL be able to queue a message on any task at any time. Each message SHALL get a unique prompt id, its author and queue time, and stays undelivered until a transcript `UserMessage` event with that prompt id is persisted. If the task has an active run that is not `finalizing`, the status SHALL become `running`; otherwise it SHALL become `queued`.

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

### Requirement: Task status vocabulary
A task SHALL have exactly one status: `queued` (pending work, no active run), `running` (a turn is in progress or about to start), `waiting` (the last turn ended; waits for a human, the run may still be alive in its keep-alive window), `blocked` (the last run waited for an answer or approval that didn't arrive within the idle deadline), `in_review` (the agent called `complete_task`; the PR waits for review), `completed` (a human marked the task done), `rate_limited`, `limit_reached` or `failed`. `waiting`, `blocked`, `in_review` and `limit_reached` form the "Waiting" group of the task list filters; `completed` and `failed` the "Done" group.

#### Scenario: Status is always one of the defined values
- **WHEN** any component writes a task status outside this set
- **THEN** the database rejects the write

#### Scenario: Message on a blocked task
- **WHEN** a message is queued on a task in status `blocked`
- **THEN** the task status becomes `queued`

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
A board member SHALL be able to mark a task done unless it is `queued` or `running`. The task SHALL become `completed`; if it has a run in `awaiting_input`, the control plane SHALL cancel that run. The change SHALL be audited with the actor. Undelivered messages stay queued; a new message reopens the task as for any `completed` task.

#### Scenario: Close a waiting task
- **WHEN** a board member marks `DF-1` in status `waiting` done
- **THEN** `DF-1` becomes `completed` and an audit entry names the board member

#### Scenario: Not while working
- **WHEN** a board member tries to mark a `running` task done
- **THEN** the request is rejected and the status does not change
