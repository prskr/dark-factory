## MODIFIED Requirements

### Requirement: ask_question
`ask_question` SHALL raise a question on the run's task with `text`, a required `kind` and its arguments (see `agent-requests`), `blocking` (default true) and `to` (`parent` or `human`; default `parent` when the task has a parent, else `human`), and return the request id, state and addressee. It SHALL NOT accept `suggested_answers`. `to: parent` without a parent SHALL produce an error result. A blocking call SHALL return `await_input_after_turn: true`.

#### Scenario: Non-blocking question
- **WHEN** the agent calls `ask_question` with text "Should I also update the docs?", kind `text` and `blocking: false`
- **THEN** the result contains the request id and state `pending`, and the run continues

#### Scenario: Blocking question
- **WHEN** the agent calls `ask_question` with kind `text` and `blocking: true`
- **THEN** the result has `await_input_after_turn: true`

#### Scenario: Blocking by default
- **WHEN** the agent calls `ask_question` with kind `text` and without `blocking`
- **THEN** the result has `await_input_after_turn: true`

#### Scenario: Single-choice question
- **WHEN** the agent calls `ask_question` with kind `single_choice` and options `pg16` "PostgreSQL 16" and `pg17` "PostgreSQL 17"
- **THEN** a pending single-choice question with both options and "Other" allowed exists

#### Scenario: Old argument
- **WHEN** the agent calls `ask_question` with `suggested_answers` and without `kind`
- **THEN** the result has `is_error: true`, names `kind` and `options`, and no question is stored

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
`answer_question` SHALL answer a pending question addressed to the run's task with `request_id` and the structured answer of its kind (`selected` and `other`, or `text`), and return the request id and state `answered`. A question that doesn't exist, isn't addressed to the run's task or isn't pending, and an answer that doesn't fit the question, SHALL produce an error result that says why and change nothing.

#### Scenario: Parent answers its subtask
- **WHEN** the agent of `DF-20` calls `answer_question` with the id of `DF-21`'s pending single-choice question and `selected: ["yes"]`
- **THEN** the result has state `answered` and `DF-21` receives the answer as a message with the option `yes` in its JSON block

#### Scenario: Answer does not fit
- **WHEN** the agent of `DF-20` calls `answer_question` with `text` for a single-choice question
- **THEN** the result has `is_error: true`, names `selected`, and the question stays `pending`

#### Scenario: Empty fields of the other kind
- **WHEN** the agent of `DF-20` calls `answer_question` for a single-choice question with `selected: ["yes"]` and `text: ""`
- **THEN** the result has state `answered` with the option `yes`

#### Scenario: Already answered by a human
- **WHEN** the agent of `DF-20` calls `answer_question` for a question a board member answered
- **THEN** the result has `is_error: true` and names the board member

#### Scenario: Question closed
- **WHEN** the agent of `DF-20` calls `answer_question` for a question closed because `DF-21` completed
- **THEN** the result has `is_error: true` and says the question was closed with reason `task-completed`

### Requirement: answer_question offered to chat runs
Chat runs SHALL be offered `answer_question` next to their other tools, `ask_question` included.

#### Scenario: Chat answers a subtask
- **WHEN** a chat run connects
- **THEN** `Assign.tools` contains `answer_question` and `ask_question`
