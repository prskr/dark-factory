# Spec Delta

## ADDED Requirements

### Requirement: Following the base branch
During prepare, after fetching, the runtime SHALL fast-forward a task branch whose tip is an ancestor of `origin/<base>` (it has no commits of its own) to `origin/<base>`, on the local branch and on the remote, for fresh and existing workspaces. A task branch with commits of its own SHALL be left as before.

#### Scenario: Reviewer sees the new round
- **WHEN** run 2 of `DF-21` (base `factory/task/DF-20`) prepares and `DF-20` pushed 2 commits since run 1, while `DF-21` has no commits of its own
- **THEN** `factory/task/DF-21` points to the tip of `origin/factory/task/DF-20`

#### Scenario: Own commits
- **WHEN** `factory/task/DF-21` has a commit that `origin/factory/task/DF-20` doesn't contain
- **THEN** prepare leaves `factory/task/DF-21` unchanged

#### Scenario: Fresh volume of a task without own commits
- **WHEN** the volume of `DF-21` is empty, `origin/factory/task/DF-21` exists without own commits and the base moved on
- **THEN** the checked-out task branch points to the tip of `origin/factory/task/DF-20`
