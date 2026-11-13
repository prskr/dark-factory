# Spec Delta

## MODIFIED Requirements

### Requirement: Marking done readies the PR
If a task marked done has a PR that isn't attached, the control plane SHALL mark it ready for review after the status change, without a summary. A forge failure SHALL be logged and SHALL NOT undo the status.

#### Scenario: Close a waiting task with a PR
- **WHEN** a board member marks `DF-1` in status `waiting` with a draft PR done
- **THEN** `DF-1` becomes `completed`, the PR is no longer a draft and gets no new comment

#### Scenario: Mark done without a PR
- **WHEN** a board member marks a task without a PR done
- **THEN** the task becomes `completed` and the forge is not called

#### Scenario: Mark done with an attached PR
- **WHEN** a board member marks `DF-15`, attached to a human's draft PR #45, done
- **THEN** `DF-15` becomes `completed` and the forge is not called

## ADDED Requirements

### Requirement: Reopening needs the branch
A message that would move a task in status `completed`, `failed` or `cancelled` back to `queued` SHALL be rejected with a conflict naming the other task if that task now holds the same branch. Nothing SHALL be queued.

#### Scenario: Branch taken over
- **WHEN** `DF-13` on `renovate/serde-1.x` was cancelled, `DF-16` was created on `renovate/serde-1.x`, and a board member queues a message on `DF-13`
- **THEN** the message is rejected with a conflict naming `DF-16` and `DF-13` stays `cancelled`

#### Scenario: Branch free
- **WHEN** a board member queues a message on cancelled `DF-13` while no other open task holds `renovate/serde-1.x`
- **THEN** `DF-13` becomes `queued`
