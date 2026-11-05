## ADDED Requirements

### Requirement: Readable system authors
Messages authored by a system principal SHALL show a readable author name in the transcript and the queued message stack, never the reason code. The agent SHALL receive the same name in its author line. A reason without a defined name SHALL show "Dark Factory".

#### Scenario: Assignment message
- **WHEN** a board member opens a task whose first message was queued by the system with reason `task.assigned`
- **THEN** the message's author reads "Task assignment"

#### Scenario: Unknown reason
- **WHEN** a message was queued by a system principal with a reason that has no defined name
- **THEN** its author reads "Dark Factory"
