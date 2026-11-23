## ADDED Requirements

### Requirement: Generate a task title
The inline title rename of the transcript header SHALL have a button at the right end of the title input, labeled "Generate title from the description" (wand icon), that runs a `task-title` minion call with the task description. The answer SHALL replace the text in the title input without storing it; the board member saves it with the rename controls (source `user`) or cancels.

#### Scenario: Title after a failed job
- **WHEN** the minion is on, the `task-title` job of DF-15 was given up, and a board member opens the rename and presses the button
- **THEN** the title input shows a generated title, and saving it stores it with source `user`

#### Scenario: Cancel the suggestion
- **WHEN** a board member presses the button and then chooses cancel
- **THEN** the header shows the previous title and nothing is stored

### Requirement: Generate title button states
While its call runs, the generate title button SHALL show progress and be disabled, and the title input SHALL be read-only. A failed call SHALL show "The factory minion could not write a title. Try again." under the input and keep the input unchanged. The button SHALL be disabled with a tooltip naming the setting while the minion is off, and disabled while the task description is empty.

#### Scenario: Empty description
- **WHEN** the minion is on and a board member opens the rename of a task with an empty description
- **THEN** the button is disabled

#### Scenario: Call fails
- **WHEN** the minion's provider is down and a board member presses the button
- **THEN** the error text is shown under the title input and the input keeps its text

#### Scenario: Minion off
- **WHEN** the minion is off and a board member opens the rename of DF-15
- **THEN** the button is disabled and its tooltip says that the factory minion is off
