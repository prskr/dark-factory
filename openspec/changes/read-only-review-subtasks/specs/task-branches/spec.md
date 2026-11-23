# Spec Delta

## ADDED Requirements

### Requirement: Read-only tasks hold no branch
A task with a read-only workspace SHALL have no task branch and no WIP branch, and SHALL NOT hold a branch for the "One open task per branch" rule. No branch named after its key SHALL be created on the forge.

#### Scenario: Branches after a review
- **WHEN** read-only task `DF-21` (base task `DF-20`) has run twice and is Done
- **THEN** neither `factory/task/DF-21` nor `factory/wip/DF-21` exists on the forge

#### Scenario: Base task's branch stays free
- **WHEN** read-only task `DF-21` reviews `renovate/serde-1.x` of `DF-13`, which is then cancelled, and a task is created on `renovate/serde-1.x`
- **THEN** the new task is created; `DF-21` doesn't hold the branch
