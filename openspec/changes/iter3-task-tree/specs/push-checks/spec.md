# Spec Delta

## MODIFIED Requirements

### Requirement: Checked commits
Checks SHALL run before forwarding, on the commits a push adds: reachable from a pushed new SHA, not reachable from an upstream ref. WIP branches and other tasks' task branches, except the run's base branch, SHALL NOT count as upstream, so their commits are checked again for this task, including the identity of human commits on them. The run's base branch SHALL count as upstream. A merge commit SHALL count as changing only the paths that differ from every parent. Other commits already on the forge SHALL NOT be checked again.

#### Scenario: Rebased onto a new base
- **WHEN** the task branch is rebased onto a `main` that contains human commits and pushed
- **THEN** only the agent's rebased commits are checked

#### Scenario: WIP commit pushed to the task branch
- **WHEN** a commit that changes `.dark-factory/config.toml` was pushed to the WIP branch and is then pushed to the task branch
- **THEN** the task-branch push is rejected with `df:protected-path`

#### Scenario: Merging the base branch
- **WHEN** a human changed `.forgejo/workflows/ci.yml` on `main` and the agent merges `main` into its task branch with linear history off
- **THEN** the push produces no `df:protected-path` finding for that path

#### Scenario: Task based on another task pushes
- **WHEN** a task based on another task pushes, and its branch contains the forge's merge commit (forge identity) from the base task's branch
- **THEN** the push produces no `df:identity` finding for that commit
