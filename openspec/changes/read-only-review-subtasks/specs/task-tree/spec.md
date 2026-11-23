# Spec Delta

## ADDED Requirements

### Requirement: Read-only tasks need a base task and are none
A task of kind `task` with a read-only workspace SHALL have a base task. A task with a read-only workspace, chats included, SHALL NOT be the base task of another task: `create_task` naming one as `base_task` SHALL produce the invalid base task error and create nothing. A read-only task's own base task MAY be the caller's task.

#### Scenario: Read-only task as base task
- **WHEN** an agent calls `create_task` with `base_task: "DF-21"` and `DF-21` is a read-only task
- **THEN** the result has `is_error: true` naming `DF-21` and no task is created

#### Scenario: Review of the caller's own branch
- **WHEN** the agent of `DF-30` calls `create_task` with `base_task: "DF-30"` and `read_only: true`
- **THEN** a read-only task with parent `DF-30` and base task `DF-30` exists

### Requirement: Children of a read-only task
A task with a read-only workspace SHALL be able to create tasks with a writable workspace, as a chat can. Their effective base branch follows their own base task, never the read-only parent.

#### Scenario: Reviewer delegates a fix
- **WHEN** the agent of read-only task `DF-21` calls `create_task` with `base_task: "DF-20"` and without `read_only`
- **THEN** a writable task `DF-22` with parent `DF-21`, base task `DF-20` and task branch `factory/task/DF-22` exists
