## ADDED Requirements

### Requirement: Collapsible task description
The "Description" section of the task details SHALL be collapsible by its header, and the header SHALL start with a chevron like the Runs and Dependencies headers: pointing right while collapsed and down while expanded. The browser's default disclosure marker SHALL NOT be shown.

#### Scenario: Collapsed description
- **WHEN** a task with a description is opened
- **THEN** the "Description" header shows a chevron pointing right

#### Scenario: Expanded description
- **WHEN** a board member activates the "Description" header
- **THEN** the description is shown and the chevron points down

### Requirement: Project and persona links in the task details
In the task details (the details column and the bottom sheet on phones), the project value SHALL link to the project's settings page `/projects/{key}`, and the persona value SHALL link to the persona list `/personas`.

#### Scenario: Open the project from a task
- **WHEN** a board member on `DF-21` chooses the project "Dark Factory" in the task details
- **THEN** `/projects/DF` opens

#### Scenario: Open the personas from a task
- **WHEN** a board member on `DF-21` chooses the persona "Linus" in the task details
- **THEN** `/personas` opens
