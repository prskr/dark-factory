## ADDED Requirements

### Requirement: Selected details tab marked
The selected tab of the details column, on the task page and on the chat page, SHALL be marked by a 2 px accent line below its label and medium weight, like the settings page tabs; the other tabs SHALL be muted and regular weight. A tab's count SHALL be a plain muted number after its label.

#### Scenario: Artifacts tab open
- **WHEN** a board member opens the Artifacts tab of `DF-12`, which has 2 subtasks and 3 artifacts
- **THEN** "Artifacts 3" has the accent line and medium weight, "Overview" and "Subtasks 2" are muted without a line, and the counts are plain numbers
