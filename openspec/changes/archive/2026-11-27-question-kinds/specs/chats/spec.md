## MODIFIED Requirements

### Requirement: Tools of a chat run
A chat run SHALL be offered `set_task_title`, `create_task`, `list_personas`, `send_message`, `report_progress`, `ask_question`, `request_approval`, `write_artifact`, `list_artifacts`, `read_artifact`, `list_issues` and `create_issue`, and SHALL NOT be offered `complete_task`, `update_pull_request` or `comment_on_issue`. A call to a tool not offered SHALL be answered with a tool error and change nothing.

#### Scenario: Delegation from a chat
- **WHEN** the agent of `DF-12-CHAT` calls `create_task` for persona Linus
- **THEN** a task is created whose parent is `DF-12-CHAT`

#### Scenario: Completing a chat
- **WHEN** the agent of `DF-12-CHAT` calls `complete_task`
- **THEN** the call returns a tool error and the chat's status is unchanged

#### Scenario: Issues from a chat
- **WHEN** the agent of `DF-12-CHAT` calls `list_issues`
- **THEN** the call returns the open issues of the project's repository

#### Scenario: Question from a chat
- **WHEN** the agent of `DF-12-CHAT` calls `ask_question` with kind `single_choice` and two options
- **THEN** a pending question of `DF-12-CHAT` exists, its card is on the chat timeline and the inbox lists it with a link to `/chats/DF-12-CHAT`

### Requirement: Archiving a chat
Archiving SHALL record when and by whom a chat was archived, keep its status, and be rejected while the chat is `queued` or `running` or has an open approval request. It SHALL close the chat's own pending questions with reason `chat-archived`. Archiving SHALL release a run of the chat that waits in its keep-alive window. Archiving SHALL NOT change the chat's subtasks.

#### Scenario: Archive an idle chat
- **WHEN** a board member archives `DF-12-CHAT` in status `waiting`
- **THEN** the chat is archived, its status stays `waiting` and an audit entry `chat.archive` names the board member

#### Scenario: Archive with an idle run
- **WHEN** a board member archives `DF-12-CHAT` in status `waiting` while its run waits in its keep-alive window after a turn
- **THEN** the run is released and the chat's status stays `waiting`

#### Scenario: Archive while running
- **WHEN** a board member archives `DF-12-CHAT` while its run is active
- **THEN** the request is rejected and the chat is not archived

#### Scenario: Archive with an open approval request
- **WHEN** `DF-12-CHAT` has a pending approval request and a board member archives it
- **THEN** the request is rejected with a reason naming the open request

#### Scenario: Archive with a pending question
- **WHEN** `DF-12-CHAT` has a pending question and a board member archives it
- **THEN** the chat is archived, the question is `closed` with reason `chat-archived` and the inbox no longer lists it

#### Scenario: Running subtask
- **WHEN** a board member archives `DF-12-CHAT` while its subtask `DF-20` is running
- **THEN** the chat is archived and `DF-20` keeps running

## ADDED Requirements

### Requirement: A message leaves a chat question open
A board member's message on a chat with a pending question SHALL be delivered as usual and SHALL NOT answer or close the question; the question SHALL stay answerable.

#### Scenario: Reply instead of answer
- **WHEN** `DF-12-CHAT` has a pending single-choice question and the board member writes "Let me think about it" in the message box
- **THEN** the message is delivered to the agent and the question card still offers its options and "Answer"
