## ADDED Requirements

### Requirement: Readable system authors
Messages authored by a system principal SHALL show a readable author name in the transcript and the queued message stack, never the reason code. The agent SHALL receive the same name in its author line. A reason without a defined name SHALL show "Dark Factory".

#### Scenario: Assignment message
- **WHEN** a board member opens a task whose first message was queued by the system with reason `task.assigned`
- **THEN** the message's author reads "Task assignment"

#### Scenario: Unknown reason
- **WHEN** a message was queued by a system principal with a reason that has no defined name
- **THEN** its author reads "Dark Factory"

## MODIFIED Requirements

### Requirement: Queued message stack
Every undelivered message of the task, whether authored by a human, an agent or the system, SHALL be shown as a stack directly above the message input, oldest first, with their queue time, not in the transcript. A row of an agent or system author SHALL be prefixed with the author's readable name (`<name>: `); a row of a human author SHALL have no prefix. Only a human's held-back messages SHALL be editable and removable. The stack SHALL have a header "N queued · <persona> reads them at the next turn" (for a `failed` or `completed` task: "N queued · sent with the next run"). It SHALL show at most three rows and scroll inside itself, so the message input keeps its position. A delivered message SHALL leave the stack and appear in the transcript as a normal message.

#### Scenario: Message while the agent works
- **WHEN** a board member sends a message during a turn
- **THEN** the message appears in the stack above the input, not in the transcript

#### Scenario: Message picked up
- **WHEN** the next turn starts with that message
- **THEN** it leaves the stack and appears in the transcript as the board member's message without a reload

#### Scenario: Many queued messages
- **WHEN** seven messages are queued
- **THEN** the header reads "7 queued · Linus reads them at the next turn", three rows are visible, the list scrolls to reach the others, and the message input is not pushed down

#### Scenario: Author prefix of non-human rows
- **WHEN** the stack holds a message queued by the agent "Linus", a message queued by the system with reason `task.assigned`, and a board member's message
- **THEN** the first row starts with "Linus: ", the second with "Task assignment: ", the third has no prefix, and neither the agent's nor the system's row offers edit or remove