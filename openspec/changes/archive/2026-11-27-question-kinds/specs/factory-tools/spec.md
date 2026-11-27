## MODIFIED Requirements

### Requirement: ask_question
`ask_question` SHALL raise a question for humans on the run's task with `text`, a required `kind` (`single_choice`, `multi_choice` or `text`), the kind's arguments (`options`, `allow_other`, `min_selected`, `max_selected`) and `blocking` (default true), and return the request id and state. It SHALL NOT accept `suggested_answers`. A blocking call SHALL return a result with `await_input_after_turn: true`.

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

#### Scenario: Limits on a single choice
- **WHEN** the agent calls `ask_question` with kind `single_choice`, two options and `max_selected: 2`
- **THEN** the result has `is_error: true` and names `max_selected`

### Requirement: ask_question ignores neutral arguments of another kind
An argument that does not apply to the kind SHALL be ignored when it is neutral (`null`, an empty `options` list, `allow_other: false`) and SHALL produce an error result otherwise.

#### Scenario: Text question with empty options
- **WHEN** the agent calls `ask_question` with kind `text`, `options: []`, `allow_other: false` and `min_selected: null`
- **THEN** a pending text question exists

### Requirement: ask_question reports a merged question
When an identical pending question is returned instead of a new one, the result SHALL say so (`merged: true`) and that its kind and options are unchanged.

#### Scenario: Same text with other options
- **WHEN** the agent asks "Which version?" as a single choice with options `a` and `b` while the same text is pending as a text question
- **THEN** the result has the pending question's id, `merged: true`, and the question stays a text question

### Requirement: ask_question describes the answer
The description of `ask_question` SHALL say when to use each kind, that option ids come back in the answer, that the answer arrives as a message with a JSON block holding the structured answer, and that questions block by default and `blocking: false` is for questions the agent can work around.

#### Scenario: Description read by the agent
- **WHEN** a task run connects
- **THEN** the description of `ask_question` in `Assign.tools` names the three kinds, the JSON answer block and that questions block by default
