## MODIFIED Requirements

### Requirement: Request kinds
A request SHALL be either a question (text and a question kind) or an approval request (type from the closed list of registered types, typed payload, reason). Each request SHALL belong to one task, record who raised it (agent run or control plane) and whether it is blocking.

#### Scenario: Question stored
- **WHEN** the agent of run R on `DF-7` asks "Use Postgres 16 or 17?" as a single-choice question with options "16" and "17"
- **THEN** a pending single-choice question for `DF-7` raised by R with both options exists

#### Scenario: Unknown approval type
- **WHEN** an approval request names type `delete-everything`
- **THEN** it is rejected and nothing is stored

### Requirement: Decisions
Deciding SHALL require a human with `approval:decide` (iteration 2: the `board-member` role). An answer SHALL carry a structured answer that fits the question ("Answer validation"). An approval SHALL carry one of the scopes the type offers. A denial MAY carry a comment.

#### Scenario: Answer by suggested answer
- **WHEN** a board member chooses the option "17" of a pending question that was migrated from the suggested answers "16" and "17", and answers
- **THEN** the question is `answered` with the id of the option labelled "17" and the deciding user

#### Scenario: Answer by option
- **WHEN** a board member chooses the option `pg17` of a single-choice question and answers
- **THEN** the question is `answered` with the selected option id `pg17` and the deciding user

#### Scenario: Text answer
- **WHEN** a board member answers a text question with "Use the **old** endpoint"
- **THEN** the question is `answered` with that text and the deciding user

### Requirement: Decisions are delivered as messages
Every decision SHALL queue one message on the task, authored by the system principal with reason `request.decided`, stating the request, the outcome, the scope or answer and the comment, in the same transaction as the decision. For a question it SHALL also hold the structured answer as one JSON block. The message's content SHALL be marked hidden: the agent receives it, and the request card shows the decision to humans.

#### Scenario: Answer reaches a waiting task
- **WHEN** a question of task `DF-7` (status `blocked`) is answered
- **THEN** a `request.decided` message is queued and `DF-7` becomes `queued`

#### Scenario: Answer reaches a live run
- **WHEN** a question is answered while the run of `DF-7` is awaiting input
- **THEN** the runtime receives the message as a prompt and starts the next turn

#### Scenario: Answer not shown twice
- **WHEN** a board member opens `DF-7` after its question was answered and the answer was delivered
- **THEN** the timeline shows the question card with the answer, and no separate message with the answer

#### Scenario: Structured choice answer
- **WHEN** a board member chooses `api` and `ui` and writes "also the CLI" as Other on a multiple-choice question
- **THEN** the message holds a JSON block with the request id, kind `multi_choice`, the options `api` and `ui` with their labels, and `other` "also the CLI"

#### Scenario: Structured text answer
- **WHEN** a board member answers a text question
- **THEN** the message holds the text and a JSON block with the request id, kind `text` and the text

## ADDED Requirements

### Requirement: Question kinds
A question SHALL have one kind: `single_choice` (one of its options), `multi_choice` (several of its options, within optional `min_selected` and `max_selected`, default 1 and no maximum) or `text` (a free Markdown answer). A choice question SHALL have options and MAY allow an Other answer (default allowed); a text question SHALL have neither.

#### Scenario: Multiple choice with limits
- **WHEN** an agent asks a multiple-choice question with four options and `max_selected: 2`
- **THEN** the question is stored with minimum 1 and maximum 2

#### Scenario: Text question with options
- **WHEN** an agent asks a text question with options
- **THEN** it is rejected and nothing is stored

### Requirement: Question options
A choice question SHALL have 2 to 10 options. Each option SHALL have an id (lowercase letters, digits, `-` and `_`, starting with a letter or digit, at most 40 characters) unique in the question, a one-line plain-text label of 1 to 200 characters unique in the question ignoring case, and MAY have a one-line plain-text description of at most 300 characters.

#### Scenario: One option
- **WHEN** an agent asks a single-choice question with one option
- **THEN** it is rejected and nothing is stored

#### Scenario: Label with a line break
- **WHEN** an option label contains a newline
- **THEN** the question is rejected and nothing is stored

### Requirement: Answer validation
An answer SHALL be rejected, naming the field, and nothing stored when it does not fit the question: a single-choice answer that is not exactly one known option or Other; a multiple-choice answer with unknown or repeated options or a count (Other included) outside its limits; Other on a question without Other, or an empty Other; a text answer that is empty or over 4,000 characters.

#### Scenario: Two options on a single choice
- **WHEN** an answer to a single-choice question names two options
- **THEN** it is rejected with "Choose one option." and the question stays `pending`

#### Scenario: Too many options
- **WHEN** an answer to a multiple-choice question with `max_selected: 2` names three options
- **THEN** it is rejected with "Choose at most 2 options." and the question stays `pending`

#### Scenario: Unknown option
- **WHEN** an answer names an option id the question does not have
- **THEN** it is rejected and the question stays `pending`

#### Scenario: Empty text
- **WHEN** a text question is answered with only spaces
- **THEN** it is rejected with "Write an answer." and the question stays `pending`

### Requirement: Other answer
When a choice question allows Other, its answer MAY carry an Other text of 1 to 1,000 characters. In a single-choice answer Other SHALL replace the option; in a multiple-choice answer it SHALL count as one choice.

#### Scenario: Other on a single choice
- **WHEN** a board member answers a single-choice question with Other "Neither, use 15"
- **THEN** the question is `answered` with no option and Other "Neither, use 15"

#### Scenario: Other not allowed
- **WHEN** an answer carries Other on a question with `allow_other: false`
- **THEN** it is rejected and the question stays `pending`

### Requirement: Stored questions migrated
Questions stored before question kinds SHALL be readable after the upgrade: a question with suggested answers SHALL become a single-choice question with those answers as options and Other allowed; one without SHALL become a text question. Their answers SHALL become the matching option, Other, or the text.

#### Scenario: Answered question with a suggested answer
- **WHEN** an instance with a question answered "17" from the suggested answers "16" and "17" is upgraded
- **THEN** the question is a single-choice question answered with the option labelled "17"

#### Scenario: Free answer to a question with suggested answers
- **WHEN** an instance with a question answered "15" from the suggested answers "16" and "17" is upgraded
- **THEN** the question is answered with Other "15"
