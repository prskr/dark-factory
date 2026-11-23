# Spec Delta

## ADDED Requirements

### Requirement: Review checkout
A run of a task of kind `task` with a read-only workspace SHALL check out the current head of its base branch, which is its base task's branch while the base task is not Done, detached, fetched at the start of the run. The clone SHALL NOT be shallow: the full commit history of the reviewed branch and of the branch it is compared against SHALL be local, so `git diff <base>...HEAD`, `git merge-base`, `git log` and `git blame` work.

#### Scenario: First review
- **WHEN** the first run of read-only task `DF-21` (base task `DF-20`, project base `main`) prepares its workspace
- **THEN** `HEAD` is the head of `origin/factory/task/DF-20`, detached, and `git merge-base HEAD origin/main` succeeds

#### Scenario: Review again after new commits
- **WHEN** `DF-20` pushed two commits after `DF-21`'s previous run, and a message starts a new run of `DF-21`
- **THEN** the new run's `HEAD` is the new head of `factory/task/DF-20`

#### Scenario: Reviewed branch is an existing branch
- **WHEN** a read-only task's base task holds `renovate/serde-1.x` with PR #40 into `main`
- **THEN** the run checks out `origin/renovate/serde-1.x` and compares against `main`

### Requirement: Reviewed commit recorded
After preparing a review checkout, the runtime SHALL write one info notice with code `workspace.review_checkout` to the transcript, naming the reviewed branch, the full SHA of the checked-out commit, the branch it is compared against and their merge base when one exists. Chat runs SHALL NOT write it.

#### Scenario: Notice per run
- **WHEN** a run of `DF-21` checks out `factory/task/DF-20` at `3f2a9c1e…` compared against `main`
- **THEN** the transcript of that run has one `workspace.review_checkout` notice naming `factory/task/DF-20`, `3f2a9c1e…`, `main` and the merge-base SHA

#### Scenario: Chat
- **WHEN** a run of `DF-12-CHAT` prepares its workspace
- **THEN** no `workspace.review_checkout` notice is written
