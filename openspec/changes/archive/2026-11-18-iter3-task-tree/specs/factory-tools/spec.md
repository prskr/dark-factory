# Spec Delta

## ADDED Requirements

### Requirement: merge_subtask_pr
`merge_subtask_pr` SHALL take `task` (key of one of the run's task's subtasks), merge that subtask's PR under the rules of `task-tree` ("Merging subtask PRs", "Merge outcome") and return the merged PR's URL. It is the only tool besides `send_message` that acts on another task than the run's own.

#### Scenario: Not a subtask
- **WHEN** the agent of `DF-30` calls `merge_subtask_pr` for `DF-5`, which is not its subtask
- **THEN** the result has `is_error: true` and nothing is merged

#### Scenario: Retried call
- **WHEN** the runtime re-sends `merge_subtask_pr` with the same `call_id` after a reconnect
- **THEN** the forge is asked to merge once and both requests get the same result
