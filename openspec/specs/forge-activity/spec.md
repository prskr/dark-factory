# forge-activity Specification

## Purpose

Records in a task's transcript when the task's work reached the forge (task-branch pushes, pull request opened and marked ready), from trusted sources only and exactly once, so a board member sees when it happened relative to the agent's work (ADR-0046).

## Requirements

### Requirement: Task-branch pushes become forge activity
When the gateway reports a final verdict (`accepted`, `rejected`, `upstream-failed`) for a push that updates the run's task branch, the control plane SHALL append one forge activity event to that run's transcript, in the same transaction as the push record. The event SHALL carry the ref, old and new SHA, and the verdict and reason code of the task-branch ref itself, not of the push as a whole.

#### Scenario: Accepted push
- **WHEN** the gateway records push P of run R updating `refs/heads/factory/task/DF-7` from `abc1234` to `def5678` with verdict `accepted`
- **THEN** run R's transcript has one forge activity event for P with that ref, both SHAs and verdict `accepted`

#### Scenario: Rejected push
- **WHEN** the gateway records a push to the task branch of `DF-7` with verdict `rejected` and reason `df:non-fast-forward`
- **THEN** the transcript has one forge activity event with verdict `rejected` and reason `df:non-fast-forward`

#### Scenario: Other ref refused by the forge
- **WHEN** a push updates the task branch and the WIP branch of `DF-7`, and the forge accepts the task branch but refuses the WIP branch
- **THEN** the forge activity event has verdict `accepted` for the task branch

#### Scenario: Forge unreachable
- **WHEN** a push to the task branch of `DF-7` fails because the forge is unreachable
- **THEN** the forge activity event has verdict `upstream-failed`

#### Scenario: Push still forwarding
- **WHEN** the gateway records a push as `forwarding`
- **THEN** no forge activity event is written for it

### Requirement: Other refs are not forge activity
A push that does not update the run's task branch SHALL NOT produce a forge activity event.

#### Scenario: WIP branch push
- **WHEN** the gateway records an accepted push that only updates the WIP branch of `DF-7`
- **THEN** no forge activity event is written

### Requirement: Push activity comes from the gateway only
Forge activity for pushes SHALL be written only from the gateway's push record. A push report from the runtime SHALL NOT produce a forge activity event.

#### Scenario: Runtime reports a push the gateway never saw
- **WHEN** the runtime sends a `PushReport` with status OK and no push record exists
- **THEN** no forge activity event for a push is written

### Requirement: The runtime cannot write forge activity
A forge activity event sent by the runtime on its transcript channel SHALL NOT be stored; the rest of the runtime's events SHALL be stored as usual.

#### Scenario: Forged push row
- **WHEN** the runtime sends a transcript event of kind forge activity claiming an accepted push
- **THEN** no transcript row exists for it and the run's following events are stored

### Requirement: Pull request opened becomes forge activity
When the control plane records a pull request for a task, whether it opened it or found an existing open one, it SHALL append one forge activity event with the PR number, URL and draft flag to the transcript of the run that triggered it, in the same transaction.

#### Scenario: Draft PR opened at first push
- **WHEN** run R's first push ahead of base leads to draft PR #12
- **THEN** run R's transcript has one forge activity event "PR #12 opened" with its URL and draft set

#### Scenario: Existing PR found
- **WHEN** an open PR #12 for the task branch already exists and is recorded during run R
- **THEN** run R's transcript has one forge activity event for PR #12

### Requirement: Pull request ready becomes forge activity
When the control plane marks the task's pull request ready for review after a completed run and the forge accepts the update, it SHALL append one forge activity event with the PR number and URL to that run's transcript. A failed update SHALL NOT produce the event.

#### Scenario: PR marked ready
- **WHEN** run 2 of `DF-1` completes and PR #12 is marked ready
- **THEN** run 2's transcript has one forge activity event "PR #12 ready for review"

#### Scenario: PR opened at run end
- **WHEN** run 1 of `DF-1` completes, PR #12 is opened at run end and then marked ready
- **THEN** the ready event carries PR #12's URL

#### Scenario: Forge refuses the update
- **WHEN** marking PR #12 ready fails
- **THEN** no ready event is written

### Requirement: Forge activity is written once
Each forge activity SHALL be stored at most once, however often its trigger is retried: once per push, once per opened PR, once per ready PR and run.

#### Scenario: Retried push record
- **WHEN** the gateway sends the same final verdict for push P twice
- **THEN** the transcript has one forge activity event for P

### Requirement: Forge activity is not sent to the agent
Forge activity events SHALL NOT be sent to the agent or included in anything the agent receives.

#### Scenario: Next run
- **WHEN** run 2 starts after run 1's transcript holds forge activity
- **THEN** no prompt or context of run 2 contains it
