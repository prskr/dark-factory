## ADDED Requirements

### Requirement: Collapsible task description
The "Description" section of the task details SHALL be collapsible by its header, and the header SHALL end with a chevron like the Runs and Dependencies headers: pointing right while collapsed and down while expanded. The browser's default disclosure marker SHALL NOT be shown.

#### Scenario: Collapsed description
- **WHEN** a task with a description is opened
- **THEN** the "Description" header shows a chevron pointing right

#### Scenario: Expanded description
- **WHEN** a board member activates the "Description" header
- **THEN** the description is shown and the chevron points down
