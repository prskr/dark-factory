## MODIFIED Requirements

### Requirement: Phone-ready task pages
Task list, new-task composer and task detail SHALL be usable at 390 px width: no horizontal page scrolling (code blocks scroll inside their block), touch-sized targets, no hover-only interactions. On phones the composer SHALL open as a bottom sheet.

#### Scenario: Long code line on a phone
- **WHEN** the transcript contains a 300-character code line
- **THEN** only that code block scrolls horizontally

#### Scenario: New task on a phone
- **WHEN** the floating New task button is tapped at 390 px width
- **THEN** the composer opens as a bottom sheet over the task list
