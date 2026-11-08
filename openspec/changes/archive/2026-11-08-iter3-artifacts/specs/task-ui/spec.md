# Spec Delta

## ADDED Requirements

### Requirement: Artifacts tab on the task page
Once a task has at least one artifact, the task details column SHALL have an Artifacts tab with the number of the task's artifacts, listing them as artifact cards. A task without artifacts SHALL have no Artifacts tab; when no other tab remains besides Overview, the details column SHALL keep its "Details" header without a tab bar. On phones the tab SHALL be part of the details bottom sheet.

#### Scenario: Research task
- **WHEN** a board member opens `DF-22` with the artifacts `findings.md` and `sources.md`
- **THEN** the Artifacts tab shows the count 2 and both cards

#### Scenario: No artifacts
- **WHEN** a board member opens a task without artifacts and without other tabs
- **THEN** the details column shows its "Details" header and no Artifacts tab

### Requirement: Artifacts tab appears with the first artifact
When the live transcript of an open task receives the task's first `write_artifact` call, the Artifacts tab SHALL appear without a page reload.

#### Scenario: First artifact
- **WHEN** a board member has a task without artifacts open and the agent writes `findings.md`
- **THEN** the Artifacts tab appears with the count 1 without a page reload

### Requirement: Artifact split view on the task page
From 768 px, opening an artifact from its card or from an artifact row in the transcript SHALL show it in the artifact split view next to the transcript, with the message input staying below the transcript, and SHALL collapse the details column to its strip. Closing the artifact SHALL restore the details column to the state it had before. Below 768 px opening an artifact SHALL open the artifact page full-screen.

#### Scenario: Desktop
- **WHEN** a board member opens `findings.md` of `DF-22` at 1440 px width
- **THEN** the transcript and the rendered `findings.md` are side by side, next to the task list, and the details column is collapsed to its strip

#### Scenario: Close restores details
- **WHEN** the details column was expanded before `findings.md` was opened and the board member closes the artifact
- **THEN** the details column is expanded again

#### Scenario: Phone
- **WHEN** a board member opens `findings.md` at 390 px width
- **THEN** the artifact page of `findings.md` is shown full-screen with a way back to the task

### Requirement: Collapsible task list
From 768 px, the task list column SHALL have a control that collapses it to a narrow strip and a control in the strip that expands it again. The browser SHALL remember the choice across tasks and reloads; the page SHALL work when browser storage is unavailable.

#### Scenario: Room for an artifact
- **WHEN** a board member collapses the task list at 1280 px width while an artifact is open
- **THEN** the transcript and the artifact share the freed width

#### Scenario: Collapse and reload
- **WHEN** a board member collapses the task list and reloads the page
- **THEN** the task list is still collapsed

### Requirement: Artifact rows in the transcript
A `write_artifact` call SHALL be shown as a transcript row with an artifact icon, the artifact name and its type that opens the artifact's current version. A failed call SHALL be shown as an ordinary tool row.

#### Scenario: Agent writes findings
- **WHEN** the agent of `DF-22` writes `findings.md`
- **THEN** the transcript shows a row "findings.md · Markdown" that opens the artifact

#### Scenario: Replaced later
- **WHEN** a board member opens the row of a `findings.md` that has since been replaced
- **THEN** the current content of `findings.md` is shown
