## MODIFIED Requirements

### Requirement: Requests on the task timeline
The task detail SHALL show each request of the task as a card on the timeline at the time it was raised, with its state, decision, decider and the actions for pending requests. A request card SHALL be visually distinct from messages and tool calls in both the light and the dark theme.

#### Scenario: Decided request in history
- **WHEN** a board member opens a task whose question was answered yesterday
- **THEN** the timeline shows the question card with the answer and who gave it

#### Scenario: Card stands out in the light theme
- **WHEN** a board member opens a task with a pending question in the light theme
- **THEN** the question card is set apart from the surrounding messages by the styling chosen in the request card design
