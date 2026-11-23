# Spec Delta

## ADDED Requirements

### Requirement: No pull request for a read-only task
The control plane SHALL NOT open, update, mark ready or comment on a pull request on behalf of a task with a read-only workspace: no draft PR on a push report, no ready update and no completion summary comment, neither on a PR of its own nor on its base task's PR.

#### Scenario: Review completed
- **WHEN** a run of read-only task `DF-21` (base task `DF-20` with draft PR #12) ends `completed` with summary "Two findings"
- **THEN** PR #12 stays a draft, gets no comment from the control plane, and no other PR exists for `DF-21`

#### Scenario: Push report
- **WHEN** a push report of a run of read-only task `DF-21` arrives
- **THEN** no PR is opened
