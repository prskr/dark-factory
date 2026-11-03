# Spec Delta

## Purpose

Defines which branch a task commits to: a branch created for the task or an existing branch it was started from, the rule that one open task holds a branch, the base branch of a task on an existing branch, and attaching a pull request someone else opened (ADR-0025).

## ADDED Requirements

### Requirement: Stored task branch
Every task SHALL store its task branch. A task created without an existing branch SHALL store `factory/task/<KEY>`. The runtime's workspace, the Git proxy's route, pull request creation and webhook routing SHALL use the stored branch. The WIP snapshot branch SHALL stay `factory/wip/<KEY>` for every task.

#### Scenario: New branch
- **WHEN** `DF-12` is created without an existing branch
- **THEN** its task branch is `factory/task/DF-12` and its WIP branch `factory/wip/DF-12`

#### Scenario: Existing branch
- **WHEN** `DF-13` is created on the existing branch `renovate/serde-1.x`
- **THEN** its task branch is `renovate/serde-1.x` and its WIP branch `factory/wip/DF-13`

### Requirement: One open task per branch
A branch SHALL be held by at most one task of a project whose status is not `completed`, `failed` or `cancelled`. Creating a task on a held branch SHALL fail with an error naming the task that holds it.

#### Scenario: Branch held by a task in review
- **WHEN** `DF-13` on `renovate/serde-1.x` is `in_review` and a task is created on `renovate/serde-1.x`
- **THEN** creation fails with an error naming `DF-13`

#### Scenario: Branch released
- **WHEN** `DF-13` on `renovate/serde-1.x` is `cancelled` and a task is created on `renovate/serde-1.x`
- **THEN** the new task is created on `renovate/serde-1.x`

### Requirement: Valid existing branches
An existing branch SHALL be a valid Git branch name given without `refs/heads/`. The project's base branch, branches starting with `factory/`, and combining an existing branch with a base task SHALL be rejected at creation with an error naming the reason.

#### Scenario: Base branch
- **WHEN** a task of project `DF` (base `main`) is created on branch `main`
- **THEN** creation fails and no task exists

#### Scenario: Factory branch
- **WHEN** a task is created on branch `factory/task/DF-7`
- **THEN** creation fails and no task exists

#### Scenario: Invalid name
- **WHEN** a task is created on branch `fix/../main` or `refs/heads/fix`
- **THEN** creation fails and no task exists

#### Scenario: Together with a base task
- **WHEN** `create_task` names both `branch: "fix/login"` and `base_task: "DF-30"`
- **THEN** the call fails and no task exists

### Requirement: Missing existing branch fails the task
Whether an existing branch exists SHALL be checked when the task's first run prepares its workspace. If the branch doesn't exist on the remote, prepare SHALL fail with a reason naming the branch, and the task SHALL become `failed`.

#### Scenario: Branch deleted before the first start
- **WHEN** `DF-14` was created on `fix/login` and the branch was deleted before its first run
- **THEN** the run fails with a reason naming `fix/login` and `DF-14` is `failed`

### Requirement: Pull request attached at creation
When a task is created on an existing branch, the control plane SHALL look up the open pull requests whose head is that branch. With exactly one, the task SHALL be attached to it and the task's base branch SHALL be that pull request's base. With none, the base branch SHALL be the project's base branch. With more than one, creation SHALL fail naming them.

#### Scenario: Renovate PR
- **WHEN** a task is created on `renovate/serde-1.x`, which has open PR #40 into `main`
- **THEN** the task is attached to PR #40 and its base branch is `main`

#### Scenario: PR into a release branch
- **WHEN** a task is created on `fix/cve`, which has open PR #41 into `release/1.2`
- **THEN** the task is attached to PR #41 and its base branch is `release/1.2`

#### Scenario: Branch without a PR
- **WHEN** a task is created on `fix/login` without an open PR
- **THEN** the task has no PR and its base branch is the project's base branch

#### Scenario: Several PRs
- **WHEN** a task is created on `fix/shared`, which has open PRs #42 into `main` and #43 into `release/1.2`
- **THEN** creation fails naming #42 and #43, and no task exists

#### Scenario: Forge unreachable
- **WHEN** the forge fails while a task is created on an existing branch
- **THEN** creation fails with the forge's error and no task exists

### Requirement: Attached pull requests stay untouched
The control plane SHALL NOT change the title, body or draft state of an attached pull request. Comments by the task and a merge, which makes the task Done, SHALL work as for a pull request the control plane opened.

#### Scenario: Completion on an attached PR
- **WHEN** the agent of `DF-13`, attached to Renovate's PR #40, calls `complete_task`
- **THEN** PR #40 keeps its title, body and draft state, gets the summary as a comment, and `DF-13` is `in_review`

#### Scenario: Attached PR merged
- **WHEN** a maintainer merges PR #40
- **THEN** `DF-13` is Done
