# Spec Delta

## Purpose

Defines how tasks build on each other's branches and how the task tree is bounded: base tasks and the branch a task starts from, open subtasks that hold back their parent's completion, cancelling a subtree, limits on delegation, and merging subtask PRs by the parent's agent (ADR-0034, ADR-0041).

## ADDED Requirements

### Requirement: Base task at creation
`create_task` SHALL accept an optional `base_task` (task key). The base task SHALL be a task of the run's project that is not Done (`completed`), `failed` or `cancelled`; the caller's own task is allowed. Any other value SHALL produce an error result naming the key and create nothing. The base task SHALL NOT change after creation.

#### Scenario: Reviewer based on the implementation
- **WHEN** the agent of `DF-19` creates `DF-21` with `base_task: "DF-20"`
- **THEN** `DF-21` has base task `DF-20`

#### Scenario: Iteration on the feature branch
- **WHEN** the agent of `DF-30` creates `DF-31` with `base_task: "DF-30"`
- **THEN** `DF-31` has parent `DF-30` and base task `DF-30`

#### Scenario: Base task already done
- **WHEN** `create_task` names `base_task: "DF-5"` and `DF-5` is `completed`
- **THEN** the result has `is_error: true` naming `DF-5` and no task is created

#### Scenario: Unknown base task
- **WHEN** `create_task` names `base_task: "DF-999"`, which doesn't exist in the project
- **THEN** the result has `is_error: true` naming `DF-999` and no task is created

### Requirement: Effective base branch
A task's base branch SHALL be the task branch of its base task, or the project's base branch if it has none. If the base task is Done, the base task's own base branch SHALL be used instead, applied recursively. The base branch SHALL be the same for the runtime's workspace, the Git proxy's route and the task's PR.

#### Scenario: Based on a task
- **WHEN** a run of `DF-21` (base task `DF-20`, project base `main`) starts
- **THEN** `Workspace.base_branch` and the Git route's base branch are `factory/task/DF-20`

#### Scenario: No base task
- **WHEN** a run of `DF-20` without base task starts
- **THEN** its base branch is `main`

#### Scenario: Base task merged
- **WHEN** `DF-21`'s base task `DF-20` is Done and `DF-21` runs again
- **THEN** `DF-21`'s base branch is `main`

### Requirement: Base branch created on first start
When a task with a base task starts its first run, the control plane SHALL create the base task's task branch through the forge from the base task's own base branch, unless the branch already exists. If creating it fails, the run SHALL end `failed` with the forge's reason and the task SHALL become `failed`.

#### Scenario: Coordinator without commits
- **WHEN** `DF-31` (base task `DF-30`) starts its first run and `DF-30` has never pushed
- **THEN** `factory/task/DF-30` is created from `main` before the runtime prepares the workspace

#### Scenario: Branch already pushed
- **WHEN** `DF-21` (base task `DF-20`) starts its first run and `factory/task/DF-20` exists
- **THEN** the branch is left as is and the run starts

### Requirement: Open subtasks
A subtask SHALL count as open if its status is not `in_review`, `completed`, `failed` or `cancelled`, or if its base task is its parent, it has a PR, and it is neither `completed` nor `cancelled`.

#### Scenario: Research subtask finished
- **WHEN** subtask `DF-22` of `DF-20` has no PR and is `in_review`
- **THEN** `DF-22` is not open

#### Scenario: Iteration with an unmerged PR
- **WHEN** subtask `DF-31` of `DF-30` has base task `DF-30`, a PR and is `in_review`
- **THEN** `DF-31` is open

#### Scenario: Subtask waiting for a human
- **WHEN** subtask `DF-22` of `DF-20` is `waiting`
- **THEN** `DF-22` is open

### Requirement: Completion blocked by open subtasks
`complete_task` SHALL produce an error result listing the keys of the task's open subtasks, with their PR links where they exist, and SHALL leave the task and its run unchanged while any subtask is open. A board member SHALL still be able to mark the task done.

#### Scenario: Iteration not merged yet
- **WHEN** the agent of `DF-30` calls `complete_task` while subtask `DF-31` has an unmerged PR into `factory/task/DF-30`
- **THEN** the result has `is_error: true` naming `DF-31` and its PR, and the run doesn't end after the turn

#### Scenario: All subtasks finished
- **WHEN** the agent of `DF-20` calls `complete_task` and its only subtask `DF-22` is `in_review` without a PR
- **THEN** the call succeeds as without subtasks

#### Scenario: Board member marks done
- **WHEN** a board member marks `DF-30` done while `DF-31` is open
- **THEN** `DF-30` becomes `completed`

### Requirement: Cancelling a subtree
Cancelling a task SHALL also cancel each of its descendants that is not `completed`, `failed` or `cancelled`, in the same transaction and with the same effects as cancelling it alone, each with its own audit entry. No child-ended message SHALL be queued on a task cancelled in the same transaction; the parent of the cancelled task SHALL be woken as for any cancel.

#### Scenario: Feature cancelled
- **WHEN** a board member cancels `DF-30`, whose subtask `DF-31` is `running` and `DF-32` is `awaiting_dependencies`
- **THEN** `DF-30`, `DF-31` and `DF-32` are `cancelled`, `DF-31`'s run is stopped, and no `task.child_ended` message is queued on `DF-30`

#### Scenario: Done subtask stays done
- **WHEN** a board member cancels `DF-30`, whose subtask `DF-31` is `completed`
- **THEN** `DF-31` stays `completed`

#### Scenario: Dependent outside the subtree
- **WHEN** `DF-40`, not a descendant of `DF-30`, depends on `DF-31` and `DF-30` is cancelled
- **THEN** `DF-40` stays `awaiting_dependencies`

### Requirement: Delegation limits
`create_task` SHALL produce an error result naming the limit and create nothing if the new task would have more ancestors than `tasks.maxDepth` (instance setting, default 5), or if the caller's task already has `tasks.maxOpenSubtasks` open subtasks (instance setting, default 10).

#### Scenario: Too deep
- **WHEN** a task with 5 ancestors calls `create_task` with default limits
- **THEN** the result has `is_error: true` naming `tasks.maxDepth`

#### Scenario: Too many open subtasks
- **WHEN** `DF-30` has 10 open subtasks and calls `create_task` with default limits
- **THEN** the result has `is_error: true` naming `tasks.maxOpenSubtasks`

#### Scenario: Finished subtasks don't count
- **WHEN** `DF-30` has 10 subtasks that are all `in_review` without a PR and calls `create_task`
- **THEN** the task is created

### Requirement: Merging subtask PRs
`merge_subtask_pr` SHALL merge the PR of the given subtask through the forge if the project's `subtask_merge` is `parent-agent`, the caller is the subtask's parent and its base task, and the subtask is `in_review` with a PR. Otherwise it SHALL produce an error result saying which condition failed and change nothing.

#### Scenario: Iteration merged by the feature agent
- **WHEN** the project has `subtask_merge: parent-agent` and the agent of `DF-30` calls `merge_subtask_pr` for `DF-31` (`in_review`, PR into `factory/task/DF-30`)
- **THEN** the PR is merged

#### Scenario: Project keeps merges with humans
- **WHEN** the project has `subtask_merge: human` and the agent of `DF-30` calls `merge_subtask_pr` for `DF-31`
- **THEN** the result has `is_error: true` naming the project setting and the PR stays open

#### Scenario: Subtask still running
- **WHEN** `DF-31` is `running`
- **THEN** the result has `is_error: true` and the PR stays open

#### Scenario: Reviewer PR into the implementation branch
- **WHEN** the agent of `DF-20` calls `merge_subtask_pr` for `DF-21`, whose base task is `DF-20` but whose parent is `DF-19`
- **THEN** the result has `is_error: true` and the PR stays open

### Requirement: Merge outcome
The merge SHALL be requested for the head commit last pushed by the subtask. On success the subtask SHALL become Done (`completed`, merge time recorded, audit entry `task.done` with the calling agent) as for a merge reported by the forge, and the call SHALL return the merged PR's URL. A forge error, such as a merge conflict, a failed required check or a moved head, SHALL produce an error result with the forge's reason and change nothing.

#### Scenario: Merged
- **WHEN** `merge_subtask_pr` merges `DF-31`'s PR
- **THEN** `DF-31` is `completed`, its merge time is set, and a later merge webhook for the PR changes nothing

#### Scenario: Conflict
- **WHEN** the forge rejects the merge of `DF-31`'s PR because of a conflict
- **THEN** the result has `is_error: true` with the forge's message and `DF-31` stays `in_review`
