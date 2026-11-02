# Spec Delta

## Purpose

Defines chats: conversations of a board member with one persona in one project, stored as tasks of kind `chat`. Covers how a chat starts, its topic, what its runs may do, how it is archived and unarchived, and how chat runs share the instance's capacity.

## ADDED Requirements

### Requirement: Starting a chat
A board member SHALL be able to start a chat in a project by choosing a persona and writing a first message. The chat SHALL be a task of kind `chat` without a parent, created by that board member. The first message SHALL be queued as an ordinary message from the board member, followed by the hidden `set_task_title` instruction. Agents SHALL NOT be able to start chats.

#### Scenario: New chat
- **WHEN** a board member starts a chat in project `DF` with persona Grace and the message "Plan the Helm chart release"
- **THEN** a chat `DF-25-CHAT` exists with persona Grace, its transcript starts with that message authored by the board member, and the chat becomes `queued`

#### Scenario: Several chats with one persona
- **WHEN** persona Linus already has a chat in project `DF` and a board member starts another chat with Linus in `DF`
- **THEN** both chats exist side by side

#### Scenario: Empty first message
- **WHEN** a board member starts a chat with an empty message
- **THEN** it is rejected with a validation error and no chat is created

### Requirement: Topic
The topic of a chat SHALL be its task title. The agent SHALL set it with `set_task_title` in the first run; until then the placeholder derived from the first message SHALL be shown. A board member SHALL be able to rename a chat like a task.

#### Scenario: Topic set by the agent
- **WHEN** the agent of `DF-25-CHAT` calls `set_task_title` with "Helm chart for iteration 3"
- **THEN** the chat's topic is "Helm chart for iteration 3"

#### Scenario: Renamed by a board member
- **WHEN** a board member renames `DF-25-CHAT` to "CRD migration"
- **THEN** the topic is "CRD migration" and a later `set_task_title` call does not change it

### Requirement: Chat instructions
Every run of a chat SHALL receive the persona's system prompt followed by fixed chat instructions from the control plane. The instructions SHALL state that the agent talks to a board member, that the checkout is read-only, that work needing a branch goes into tasks created with `create_task`, and that the chat is never completed.

#### Scenario: System prompt of a chat run
- **WHEN** a run of `DF-25-CHAT` with persona Grace connects
- **THEN** its system prompt is Grace's system prompt followed by the chat instructions

#### Scenario: Task runs unchanged
- **WHEN** a run of task `DF-20` with persona Grace connects
- **THEN** its system prompt contains no chat instructions

### Requirement: Tools of a chat run
A chat run SHALL be offered `set_task_title`, `create_task`, `list_personas`, `send_message`, `report_progress` and `request_approval`, and SHALL NOT be offered `complete_task`, `ask_question`, `update_pull_request` or `comment_on_issue`. A call to a tool not offered SHALL be answered with a tool error and change nothing.

#### Scenario: Delegation from a chat
- **WHEN** the agent of `DF-12-CHAT` calls `create_task` for persona Linus
- **THEN** a task is created whose parent is `DF-12-CHAT`

#### Scenario: Completing a chat
- **WHEN** the agent of `DF-12-CHAT` calls `complete_task`
- **THEN** the call returns a tool error and the chat's status is unchanged

### Requirement: Read-only checkout
A chat run SHALL work in a checkout of the project's base branch, reset to the remote base branch at the start of every run, discarding local changes. A chat SHALL have no task branch, no WIP branch and no pull request.

#### Scenario: Base branch moved between runs
- **WHEN** `main` gained a commit after the previous run of `DF-12-CHAT`, which had left an edited file
- **THEN** the next run starts at the new `origin/main` without the edit

#### Scenario: No branch for a chat
- **WHEN** runs of `DF-12-CHAT` have ended
- **THEN** no branch `factory/task/DF-12-CHAT` or `factory/wip/DF-12-CHAT` exists on the forge

### Requirement: Actions on a chat
A board member SHALL be able to stop a chat's run and to archive and unarchive a chat. Cancelling a chat and marking it done SHALL be rejected.

#### Scenario: Stop
- **WHEN** a board member stops `DF-12-CHAT` while its run is active
- **THEN** the run is stopped as for a task

#### Scenario: Cancel rejected
- **WHEN** a board member tries to cancel `DF-12-CHAT`
- **THEN** the request is rejected and nothing changes

### Requirement: Archiving a chat
Archiving SHALL record when and by whom a chat was archived, keep its status, and be rejected while the chat is `queued` or `running` or has an open approval request. Archiving SHALL NOT change the chat's subtasks.

#### Scenario: Archive an idle chat
- **WHEN** a board member archives `DF-12-CHAT` in status `waiting`
- **THEN** the chat is archived, its status stays `waiting` and an audit entry `chat.archive` names the board member

#### Scenario: Archive while running
- **WHEN** a board member archives `DF-12-CHAT` while its run is active
- **THEN** the request is rejected and the chat is not archived

#### Scenario: Archive with an open approval request
- **WHEN** `DF-12-CHAT` has a pending approval request and a board member archives it
- **THEN** the request is rejected with a reason naming the open request

#### Scenario: Running subtask
- **WHEN** a board member archives `DF-12-CHAT` while its subtask `DF-20` is running
- **THEN** the chat is archived and `DF-20` keeps running

### Requirement: Archived chats are read-only
An archived chat SHALL take no messages from board members and SHALL NOT start runs. A subtask that ends SHALL still queue its child-ended message on an archived chat.

#### Scenario: Message to an archived chat
- **WHEN** a board member sends a message to archived `DF-12-CHAT`
- **THEN** the request is rejected and nothing is queued

#### Scenario: Subtask ends while archived
- **WHEN** subtask `DF-20` of archived `DF-12-CHAT` completes
- **THEN** a child-ended message is queued on `DF-12-CHAT`, the chat stays archived and no run starts

### Requirement: Unarchiving a chat
Unarchiving SHALL clear the archive record and write an audit entry `chat.unarchive`. If messages are queued on the chat, it SHALL become `queued` and start a run under the chat concurrency limit.

#### Scenario: Unarchive with waiting messages
- **WHEN** a board member unarchives `DF-12-CHAT` with one queued child-ended message
- **THEN** the chat is no longer archived and a run starts that delivers the message

#### Scenario: Unarchive without messages
- **WHEN** a board member unarchives `DF-12-CHAT` with no queued messages
- **THEN** the chat is no longer archived, keeps its status and no run starts

### Requirement: Chats are never deleted
Chats SHALL NOT be deletable; their transcripts, artifacts and the parent references of their subtasks SHALL remain.

#### Scenario: No delete action
- **WHEN** a board member looks for a way to delete `DF-12-CHAT`
- **THEN** neither the UI nor the HTTP endpoints offer one

### Requirement: Chat concurrency limit
Active chat runs SHALL count against `runs.maxConcurrentChats` (instance-wide, default 2, at least 1) and not against `runs.maxConcurrent`. A queued chat SHALL start only while fewer chat runs than the limit are active.

#### Scenario: Tasks don't block chats
- **WHEN** `runs.maxConcurrent` runs of tasks are active and a chat becomes queued while no chat run is active
- **THEN** the chat's run starts

#### Scenario: Chat limit reached
- **WHEN** `runs.maxConcurrentChats` is 2, two chat runs are in a turn and a third chat becomes queued
- **THEN** the third chat stays `queued` until one of the runs ends

### Requirement: Idle chat runs give up their slot
When a chat is queued and no chat slot is free, the control plane SHALL release the chat run that has waited longest in its keep-alive window after a turn. Its chat SHALL keep its status, and the queued chat SHALL start in the freed slot.

#### Scenario: Release the longest idle chat
- **WHEN** both chat slots hold runs waiting after a turn, idle for 2 and 8 minutes, and a third chat becomes queued
- **THEN** the run idle for 8 minutes is released and the third chat's run starts

#### Scenario: No idle run
- **WHEN** both chat slots hold runs in a turn and a third chat becomes queued
- **THEN** no run is released and the third chat waits

### Requirement: Chat audit
Starting a chat SHALL write an audit entry `chat.create` with the board member, project, persona and chat key.

#### Scenario: Audit on start
- **WHEN** a board member starts `DF-25-CHAT`
- **THEN** an audit entry `chat.create` names the board member, `DF`, the persona and `DF-25-CHAT`
