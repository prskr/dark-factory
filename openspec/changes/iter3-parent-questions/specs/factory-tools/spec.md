## MODIFIED Requirements

### Requirement: ask_question
`ask_question` SHALL raise a question on the run's task with `text`, optional `suggested_answers` (at most 5), `blocking` (default false) and `to` (`parent` or `human`; default `parent` when the task has a parent task, otherwise `human`), and return the request id, state and addressee. `to: parent` on a task without a parent SHALL produce an error result. A blocking call SHALL return a result with `await_input_after_turn: true`.

#### Scenario: Non-blocking question
- **WHEN** the agent calls `ask_question` with text "Should I also update the docs?" and `blocking: false`
- **THEN** the result contains the request id and state `pending`, and the run continues

#### Scenario: Blocking question
- **WHEN** the agent calls `ask_question` with `blocking: true`
- **THEN** the result has `await_input_after_turn: true`

#### Scenario: Default addressee of a subtask
- **WHEN** the agent of `DF-21` (parent `DF-20`) calls `ask_question` without `to`
- **THEN** the result names `DF-20` as the addressee

#### Scenario: Subtask asks a human
- **WHEN** the agent of `DF-21` calls `ask_question` with `to: human`
- **THEN** the question is addressed to humans and appears in the inbox

#### Scenario: No parent
- **WHEN** the agent of a task without a parent calls `ask_question` with `to: parent`
- **THEN** the result has `is_error: true` and no question is stored

#### Scenario: Parent has ended
- **WHEN** the agent of `DF-21` calls `ask_question` without `to` while `DF-20` is Done
- **THEN** the result says the parent has ended and names humans as the addressee

## ADDED Requirements

### Requirement: answer_question
`answer_question` SHALL answer a pending question addressed to the run's task with `request_id` and `answer` (1 to 4,000 characters), and return the request id and state `answered`. A question that doesn't exist, isn't addressed to the run's task or isn't pending SHALL produce an error result that says why (not addressed to this task, already answered by whom, closed with its reason) and change nothing.

#### Scenario: Parent answers its subtask
- **WHEN** the agent of `DF-20` calls `answer_question` with the id of `DF-21`'s pending question and answer "yes"
- **THEN** the result has state `answered` and `DF-21` receives the answer as a message

#### Scenario: Already answered by a human
- **WHEN** the agent of `DF-20` calls `answer_question` for a question a board member answered
- **THEN** the result has `is_error: true` and names the board member

#### Scenario: Question closed
- **WHEN** the agent of `DF-20` calls `answer_question` for a question closed because `DF-21` completed
- **THEN** the result has `is_error: true` and says the question was closed with reason `task-completed`

### Requirement: answer_question offered to chat runs
Chat runs SHALL be offered `answer_question` next to their other tools, and SHALL still not be offered `ask_question`.

#### Scenario: Chat answers a subtask
- **WHEN** a chat run connects
- **THEN** `Assign.tools` contains `answer_question` and not `ask_question`
