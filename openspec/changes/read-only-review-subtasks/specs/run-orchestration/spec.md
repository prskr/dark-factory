# Spec Delta

## ADDED Requirements

### Requirement: Assign for read-only task runs
`Assign` of a run of a task of kind `task` with a read-only workspace SHALL be read-only with the base branch and no task or WIP branch, SHALL name in `review_base_branch` the effective base branch of its base task, SHALL contain only the tools a read-only task is offered, and SHALL carry the persona's system prompt followed by the read-only task instructions. Chat runs SHALL have an empty `review_base_branch`.

#### Scenario: Assign for DF-21
- **WHEN** a run of read-only task `DF-21` (base task `DF-20`, project base `main`) connects
- **THEN** `Assign.workspace` is read-only with base branch `factory/task/DF-20`, `review_base_branch` `main` and no task or WIP branch, and the system prompt ends with the read-only task instructions naming both branches

#### Scenario: Base task done
- **WHEN** `DF-20` was merged into `main` and a message starts a new run of `DF-21`
- **THEN** `Assign.workspace` has base branch `main` and `review_base_branch` `main`, and the instructions say that `DF-20` is done

### Requirement: Read-only task instructions
The read-only task instructions SHALL state that the checkout is read-only and pushes are rejected, name the reviewed branch and the branch to compare against, say that a later message in the same run needs a fetch to see new commits, and say how to report: comments on the base task's PR, artifacts and `complete_task`.

#### Scenario: Instructions of DF-21
- **WHEN** a run of read-only task `DF-21` (base task `DF-20` with PR #12, project base `main`) connects
- **THEN** its instructions name `factory/task/DF-20` and `main`, say that pushes are rejected and point to `comment_on_pull_request`, `write_artifact` and `complete_task`

### Requirement: Run end of a read-only task
When a run of a task of kind `task` with a read-only workspace ends with outcome `completed`, the task SHALL become `completed` (Done) instead of `in_review`. The rules for undelivered prompts, human status and cancelled tasks ("Run end with undelivered prompts or a human status") SHALL apply unchanged. All other outcomes SHALL map as for other tasks.

#### Scenario: Review completed
- **WHEN** a run of read-only task `DF-21` ends `completed`
- **THEN** `DF-21` is `completed`

#### Scenario: Completed with an undelivered message
- **WHEN** a parent's message is queued on `DF-21` during the turn in which its agent calls `complete_task`
- **THEN** the run ends `completed` and `DF-21` becomes `queued`

#### Scenario: Turn ended without completion
- **WHEN** a run of `DF-21` ends `end_turn`
- **THEN** `DF-21` is `waiting`

### Requirement: Read-only tasks run in task slots
Runs of tasks of kind `task` with a read-only workspace SHALL count against `runs.maxConcurrent`, not `runs.maxConcurrentChats`, SHALL use the keep-alive window of tasks, and SHALL NOT be released to free a chat slot.

#### Scenario: Chat slots full
- **WHEN** all chat slots are taken, a task slot is free and read-only task `DF-21` becomes queued
- **THEN** `DF-21`'s run starts

#### Scenario: Task slots full
- **WHEN** `runs.maxConcurrent` runs of tasks are active, a chat slot is free and `DF-21` becomes queued
- **THEN** `DF-21` stays `queued` until a task slot frees
