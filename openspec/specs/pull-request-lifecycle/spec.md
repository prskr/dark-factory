# pull-request-lifecycle Specification

## Purpose

The control plane, not the agent, opens exactly one pull request per task with commits, always against the right base, and marks it ready when the task completes (ADR-0025).

## Requirements

### Requirement: Draft PR on first push ahead of base
On a successful task-branch `PushReport` with at least one commit ahead of base, if the task has no PR, the control plane SHALL open a draft PR from the task branch into the project's base branch, using stored title/body or defaults (`<KEY>: <task title>`, task description plus a `Task: <KEY>` line).

#### Scenario: First push
- **WHEN** the runtime reports pushing `factory/task/DF-1` 2 commits ahead of `main`
- **THEN** a draft PR `factory/task/DF-1 → main` exists and its number and URL are stored on the task

#### Scenario: Push without commits ahead
- **WHEN** a push report shows 0 commits ahead
- **THEN** no PR is opened

### Requirement: Idempotent creation
If an open PR for the same head and base already exists (creation conflict, or a crash between creating and recording), the control plane SHALL record that PR instead of opening a second one.

#### Scenario: Crash after creation
- **WHEN** the PR was created but not recorded and the next push report arrives
- **THEN** the existing PR is found and recorded; still exactly one PR exists

### Requirement: Failed creation is retried
If opening the PR fails, the control plane SHALL retry on the next push report and at run end, without failing the run.

#### Scenario: Forge down at first push
- **WHEN** PR creation fails at the first push and succeeds at the second
- **THEN** exactly one draft PR exists after the second push

### Requirement: Ready for review on completion
When a run ends with outcome `completed` and the task has a PR, the control plane SHALL mark the PR ready for review and post the completion summary as a PR comment.

#### Scenario: Task completed
- **WHEN** run 2 of `DF-1` ends with `completed`
- **THEN** the PR is no longer a draft and carries the summary as a comment

### Requirement: Divergence recorded
A task-branch push rejected as non-fast-forward, or a run report with `diverged`, SHALL mark the task as diverged.

#### Scenario: Remote moved
- **WHEN** a push report has status `REJECTED_NON_FAST_FORWARD`
- **THEN** the task is flagged diverged
