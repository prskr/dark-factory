## MODIFIED Requirements

### Requirement: Pending requests page
The UI SHALL list all pending requests addressed to humans across tasks, newest first, with task key and title, kind, type, the agent's text or reason and the time raised, and SHALL update live when requests change. A question readdressed to humans SHALL be marked with the parent task that ended. Questions addressed to a parent task SHALL NOT be listed.

#### Scenario: New request appears
- **WHEN** an agent raises a question while a board member has the inbox open
- **THEN** the question appears without a page reload

#### Scenario: Question to a parent not listed
- **WHEN** the agent of `DF-21` asks its parent `DF-20` a question
- **THEN** the inbox doesn't list it

#### Scenario: Readdressed question appears
- **WHEN** `DF-20` is cancelled while `DF-21`'s question to it is pending
- **THEN** the inbox lists the question with a note that its parent `DF-20` ended, without a page reload

### Requirement: Pending count in the navigation
The navigation SHALL show the number of pending requests addressed to humans on every page and update it live.

#### Scenario: Count after a decision
- **WHEN** one of three pending requests is decided
- **THEN** the navigation shows 2

#### Scenario: Question to a parent not counted
- **WHEN** two requests addressed to humans and one question addressed to a parent task are pending
- **THEN** the navigation shows 2

### Requirement: Requests on the task timeline
The task detail SHALL show each request of the task as a card on the timeline at the time it was raised, with its state, decision, decider and the actions for pending requests. A question card SHALL name its addressee (the parent task with its persona, or humans) and, for a readdressed question, the parent task that ended; the actions SHALL also be offered for a pending question addressed to a parent task.

#### Scenario: Decided request in history
- **WHEN** a board member opens a task whose question was answered yesterday
- **THEN** the timeline shows the question card with the answer and who gave it

#### Scenario: Question to the parent on the task page
- **WHEN** a board member opens `DF-21` while its question to `DF-20` is pending
- **THEN** the card says it was asked to `DF-20` and its persona, and offers the suggested answers and a free-text answer

#### Scenario: Answered by the parent's agent
- **WHEN** a board member opens `DF-21` after the agent of `DF-20` answered its question
- **THEN** the card shows the answer and names `DF-20` and its persona as the one who answered

#### Scenario: Readdressed question on the task page
- **WHEN** a board member opens `DF-21` after its parent `DF-20` was cancelled while `DF-21`'s question to it was pending
- **THEN** the card says the question went to humans because `DF-20` ended
