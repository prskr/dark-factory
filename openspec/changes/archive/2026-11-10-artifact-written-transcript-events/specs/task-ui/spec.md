# Spec Delta

## MODIFIED Requirements

### Requirement: Artifacts tab appears with the first artifact
When the live transcript of an open task receives the task's first artifact-written event, the Artifacts tab SHALL appear without a page reload, whatever harness the agent runs in.

#### Scenario: First artifact
- **WHEN** a board member has a task without artifacts open and the agent writes `findings.md`
- **THEN** the Artifacts tab appears with the count 1 without a page reload

#### Scenario: Harness wraps the tool
- **WHEN** a board member has a task without artifacts open and its OpenCode agent writes `findings.md` from inside its `execute` tool
- **THEN** the Artifacts tab appears with the count 1 without a page reload

### Requirement: Artifact rows in the transcript
An artifact-written event SHALL be shown as a transcript row with an artifact icon, the artifact name and its type that opens the artifact's current version. The harness's tool row of the call SHALL stay visible as an ordinary tool row; a failed call SHALL be shown by that tool row only.

#### Scenario: Agent writes findings
- **WHEN** the agent of `DF-22` writes `findings.md`
- **THEN** the transcript shows the tool row of the call and a row "findings.md · Markdown" that opens the artifact

#### Scenario: Harness wraps the tool
- **WHEN** the OpenCode agent of `DF-22` writes `findings.md` from inside its `execute` tool
- **THEN** the transcript shows the `execute` tool row and a row "findings.md · Markdown" that opens the artifact

#### Scenario: Failed write
- **WHEN** a `write_artifact` call of the agent of `DF-22` is rejected
- **THEN** the transcript shows its tool row and no artifact row

#### Scenario: Replaced later
- **WHEN** a board member opens the row of a `findings.md` that has since been replaced
- **THEN** the current content of `findings.md` is shown
