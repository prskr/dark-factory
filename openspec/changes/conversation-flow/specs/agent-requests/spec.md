## MODIFIED Requirements

### Requirement: Decisions are delivered as messages
Every decision SHALL queue one message on the task, authored by the system principal with reason `request.decided`, stating the request, the outcome, the scope or answer and the comment, in the same transaction as the decision. The message's content SHALL be marked hidden: the agent receives it, and the request card shows the decision to humans.

#### Scenario: Answer reaches a waiting task
- **WHEN** a question of task `DF-7` (status `blocked`) is answered
- **THEN** a `request.decided` message is queued and `DF-7` becomes `queued`

#### Scenario: Answer reaches a live run
- **WHEN** a question is answered while the run of `DF-7` is awaiting input
- **THEN** the runtime receives the message as a prompt and starts the next turn

#### Scenario: Answer not shown twice
- **WHEN** a board member opens `DF-7` after its question was answered and the answer was delivered
- **THEN** the timeline shows the question card with the answer, and no separate message with the answer
