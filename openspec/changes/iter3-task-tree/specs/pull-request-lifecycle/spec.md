# Spec Delta

## MODIFIED Requirements

### Requirement: Draft PR on first push ahead of base
On a successful task-branch `PushReport` with at least one commit ahead of base, if the task has no PR, the control plane SHALL open a draft PR from the task branch into the task's base branch (see `task-tree`, "Effective base branch"), using stored title/body or defaults (`<KEY>: <task title>`, task description plus a `Task: <KEY>` line). The control plane SHALL NOT open a PR for a task in status `completed` or `cancelled`, nor for a task whose branch was never pushed.

#### Scenario: First push
- **WHEN** the runtime reports pushing `factory/task/DF-1` 2 commits ahead of `main`
- **THEN** a draft PR `factory/task/DF-1 → main` exists and its number and URL are stored on the task

#### Scenario: Push of a task with a base task
- **WHEN** the runtime reports pushing `factory/task/DF-31` (base task `DF-30`) 1 commit ahead of `factory/task/DF-30`
- **THEN** a draft PR `factory/task/DF-31 → factory/task/DF-30` exists

#### Scenario: Push without commits ahead
- **WHEN** a push report shows 0 commits ahead
- **THEN** no PR is opened

#### Scenario: Cancelled before the first PR
- **WHEN** a board member cancels `DF-1` without a PR and its stopped run reports a push 2 commits ahead of `main`
- **THEN** no PR is opened and `DF-1` stays `cancelled`

#### Scenario: Marked done before the first PR
- **WHEN** a board member marks `DF-1` without a PR done and its cancelled run reports a push 2 commits ahead of `main`
- **THEN** no PR is opened and `DF-1` stays `completed`

#### Scenario: Completed without a push
- **WHEN** run 1 of `DF-1` ends `completed` and the task branch was never pushed
- **THEN** the forge is not asked to open a PR and `DF-1` is `in_review` without a PR

## ADDED Requirements

### Requirement: PR after a subtask merge
After `merge_subtask_pr` merged a subtask's PR into a task branch, the control plane SHALL open the draft PR of the task owning that branch if it has none and is neither `completed` nor `cancelled`, with the same title, body and retry rules as on a push. A forge failure SHALL NOT fail the merge.

#### Scenario: First iteration merged
- **WHEN** the agent of `DF-30`, which has no PR, merges the PR of `DF-31` into `factory/task/DF-30`
- **THEN** a draft PR `factory/task/DF-30 → main` exists
