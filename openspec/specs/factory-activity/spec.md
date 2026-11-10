# factory-activity Specification

## Purpose

Records in a task's transcript what the control plane did for the task besides forge activity (artifacts written, later wake-ups and incoming events), from the control plane only and exactly once, so the transcript shows it for every harness.

## Requirements

### Requirement: Artifact writes become factory activity
When `write_artifact` stores an artifact, the control plane SHALL append one factory activity event of type "artifact written" to the transcript of the calling run, in the same transaction as the stored artifact. The event SHALL carry the artifact's name, type (`text` or `image`), MIME type and size in bytes.

#### Scenario: Findings written
- **WHEN** the agent of run R of `DF-22` writes `findings.md` with 1,204 bytes of Markdown
- **THEN** run R's transcript has one artifact-written event with name `findings.md`, type `text`, MIME type `text/markdown` and size 1204

#### Scenario: Replaced artifact
- **WHEN** the agent of `DF-22` writes `findings.md` a second time with new content
- **THEN** the transcript has a second artifact-written event for `findings.md` with the new size

#### Scenario: Harness wraps the tool
- **WHEN** an OpenCode agent calls `write_artifact` from inside its `execute` tool and the artifact is stored
- **THEN** the transcript has the artifact-written event

### Requirement: Rejected writes are not factory activity
A `write_artifact` call that stores nothing SHALL NOT produce a factory activity event.

#### Scenario: Too many artifacts
- **WHEN** a task holds the maximum number of artifacts and the agent writes a new name
- **THEN** the call returns an error result and no artifact-written event is stored

### Requirement: Factory activity is written once
Each factory activity SHALL be stored at most once, however often its trigger is retried: once per `write_artifact` call.

#### Scenario: Retried call
- **WHEN** the runtime sends the same `write_artifact` call (same call id) twice
- **THEN** the transcript has one artifact-written event for it

### Requirement: The runtime cannot write factory activity
A factory activity event sent by the runtime on its transcript channel SHALL NOT be stored; the rest of the runtime's events SHALL be stored as usual.

#### Scenario: Forged artifact row
- **WHEN** the runtime sends a transcript event of kind factory activity claiming that `secret.md` was written
- **THEN** no transcript row exists for it and the run's following events are stored

### Requirement: Factory activity is not sent to the agent
Factory activity events SHALL NOT be sent to the agent or included in anything the agent receives.

#### Scenario: Next run
- **WHEN** run 2 starts after run 1's transcript holds an artifact-written event
- **THEN** no prompt or context of run 2 contains it

### Requirement: Factory activity is live
A stored factory activity event SHALL be published to the subscribers of the task's live transcript like any other transcript event.

#### Scenario: Board member watching
- **WHEN** a board member has `DF-22` open and its agent writes `findings.md`
- **THEN** the event reaches the open page without a reload, in order with the agent's output
