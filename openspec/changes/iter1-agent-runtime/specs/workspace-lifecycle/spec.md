# Spec Delta

## Purpose

Keeps a task's work durable in Git: preparing the workspace on the task volume, pushing the task branch whenever a turn produced commits, and snapshotting uncommitted work at the end of a run (`design/agent-run.md` §6.2, §6.5, ADR-0009).

## ADDED Requirements

### Requirement: Fresh workspace
If `/data/repo/.git` doesn't exist, the runtime SHALL make a blobless clone of `Assign.workspace.repo_url`, write its own `.git/config`, check out the task branch from `origin/<task branch>` if it exists or else from `origin/<base>`, and restore files from the WIP branch if it exists.

#### Scenario: First run
- **WHEN** run 1 of `DF-1` prepares an empty volume
- **THEN** `/data/repo` is on branch `factory/task/DF-1` at `origin/main`, cloned with `--filter=blob:none`

#### Scenario: Volume lost after earlier runs
- **WHEN** the volume is empty but `factory/task/DF-1` and `factory/wip/DF-1` exist on the remote
- **THEN** the task branch is checked out and the WIP files are restored into the working tree

### Requirement: Existing workspace
If the workspace exists, the runtime SHALL rewrite `.git/config`, fetch base and task branch, push local commits missing on the remote, fast-forward if the remote is ahead, and on divergence leave both untouched and emit a `workspace.diverged` notice. It SHALL NOT run `git clean`.

#### Scenario: Unpushed commits from a killed run
- **WHEN** the previous run was killed after committing but before pushing
- **THEN** prepare pushes those commits and reports the push

#### Scenario: Diverged
- **WHEN** local and remote task branches both have commits the other lacks
- **THEN** nothing is reset, a `workspace.diverged` notice is emitted and the run report marks the workspace diverged

### Requirement: Push after every turn with new commits
After each turn whose HEAD differs from the last pushed commit, the runtime SHALL push the task branch with an explicit refspec, never forcing, and SHALL send a `PushReport` with status and commits ahead of base.

#### Scenario: Turn with commits
- **WHEN** a turn ends with 2 new commits
- **THEN** the task branch is pushed and a `PushReport { target: TASK_BRANCH, status: OK, commits_ahead_of_base: 2 }` is sent

#### Scenario: Remote moved
- **WHEN** the push is rejected as non-fast-forward
- **THEN** the report has `REJECTED_NON_FAST_FORWARD`, no force push happens and the task branch is left as is

### Requirement: Finalize
At the end of every run, including cancelled and failed ones where possible, the runtime SHALL stop the harness (graceful, then kill after a timeout), rewrite `.git/config`, push the task branch, write a WIP snapshot without touching the agent's index, HEAD or branches, and send `RunReport` with outcome, branch SHAs, push state, divergence, workspace size and harness session id.

#### Scenario: Uncommitted work
- **WHEN** the run ends with modified, uncommitted files
- **THEN** a commit containing them is force-pushed to `factory/wip/DF-1`, and `git status` in the workspace is unchanged

#### Scenario: Clean worktree
- **WHEN** the run ends with no uncommitted changes
- **THEN** the remote WIP branch is deleted if it exists and a `PushReport` with `DELETED` is sent

### Requirement: WIP snapshot excludes secrets
The WIP snapshot SHALL respect `.gitignore`, drop paths matching the secret-file denylist (`.env*`, `*.pem`, `*.key`, `id_*`, …) and skip the snapshot if it exceeds the size cap, emitting a notice.

#### Scenario: Untracked .env
- **WHEN** an untracked `.env` file exists at finalize
- **THEN** the WIP commit doesn't contain it
