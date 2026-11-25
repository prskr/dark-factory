## MODIFIED Requirements

### Requirement: ask_question
`ask_question` SHALL raise a question for humans on the run's task with `text`, a required `kind` (`single_choice`, `multi_choice` or `text`), the kind's arguments (`options`, `allow_other`, `min_selected`, `max_selected`) and `blocking` (default false), and return the request id and state. It SHALL NOT accept `suggested_answers`. A blocking call SHALL return a result with `await_input_after_turn: true`.

#### Scenario: Non-blocking question
- **WHEN** the agent calls `ask_question` with text "Should I also update the docs?", kind `text` and `blocking: false`
- **THEN** the result contains the request id and state `pending`, and the run continues

#### Scenario: Blocking question
- **WHEN** the agent calls `ask_question` with kind `text` and `blocking: true`
- **THEN** the result has `await_input_after_turn: true`

#### Scenario: Single-choice question
- **WHEN** the agent calls `ask_question` with kind `single_choice` and options `pg16` "PostgreSQL 16" and `pg17` "PostgreSQL 17"
- **THEN** a pending single-choice question with both options and "Other" allowed exists

#### Scenario: Old argument
- **WHEN** the agent calls `ask_question` with `suggested_answers` and without `kind`
- **THEN** the result has `is_error: true`, names `kind` and `options`, and no question is stored

## ADDED Requirements

### Requirement: ask_question argument errors
An `ask_question` call whose arguments break the question rules of `agent-requests` SHALL produce an error result that names the argument and the rule, and SHALL store nothing.

#### Scenario: Options on a text question
- **WHEN** the agent calls `ask_question` with kind `text` and two options
- **THEN** the result has `is_error: true`, names `options`, and no question is stored

#### Scenario: Duplicate option id
- **WHEN** the agent calls `ask_question` with kind `multi_choice` and two options with id `api`
- **THEN** the result has `is_error: true` and names the duplicate id `api`

#### Scenario: Limits that cannot be met
- **WHEN** the agent calls `ask_question` with kind `multi_choice`, three options, `allow_other: false` and `min_selected: 4`
- **THEN** the result has `is_error: true` and names `min_selected`

### Requirement: ask_question describes the answer
The description of `ask_question` SHALL say when to use each kind, that option ids come back in the answer, and that the answer arrives as a message with a JSON block holding the structured answer.

#### Scenario: Description read by the agent
- **WHEN** a task run connects
- **THEN** the description of `ask_question` in `Assign.tools` names the three kinds and the JSON answer block
