# workspace-lifecycle Specification

## Purpose

Keeps a task's work durable in Git: preparing the workspace on the task volume, pushing the task branch whenever a turn produced commits, and snapshotting uncommitted work at the end of a run (`design/agent-run.md` §6.2, §6.5, ADR-0009).

## Requirements

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

### Requirement: Transient Git failures are retried
Clone, fetch and push through the Git proxy SHALL be retried up to 3 times with delays of 1 s, 5 s and 15 s when they fail with a connection error or HTTP 503 (gateway pod without a snapshot, forge unreachable). A 403 whose body is `df: unknown source` SHALL be retried the same way, because the gateway does not know a new pod's IP until the next snapshot reaches it.

#### Scenario: Gateway rollout during clone
- **WHEN** the first clone attempt gets a 503 from a gateway pod that has no snapshot yet
- **THEN** the runtime retries and the clone succeeds against a ready pod without the run failing

#### Scenario: Forge unavailable during push
- **WHEN** a push gets 503 because the forge is unreachable and the forge is back 5 seconds later
- **THEN** a later attempt (after the 5 s or 15 s delay) succeeds and one `PushReport` with status `OK` is sent

#### Scenario: Gateway does not know the pod yet
- **WHEN** the first clone attempt of a new pod gets 403 `df: unknown source`
- **THEN** the runtime retries and the clone succeeds once the snapshot with the pod's IP has reached the gateway

### Requirement: Other Git failures are not retried
Any Git failure other than a connection error, HTTP 503 or the 403 `df: unknown source` SHALL NOT be retried, in particular rejections reported by the proxy (`remote rejected`, other 403s) and 502 (the forge rejected the proxy's credential).

#### Scenario: Credential rejected upstream
- **WHEN** a push gets 502 because the forge rejected the proxy's credential
- **THEN** the runtime doesn't retry and sends one `PushReport` with status `FAILED`

#### Scenario: Rejected push is not retried
- **WHEN** a push is rejected with `df:ref-not-allowed`
- **THEN** the runtime reports it once and doesn't push again in that turn

### Requirement: Push retry checks the remote ref
Before retrying a push the runtime SHALL re-read the remote ref; if it already equals the pushed SHA (the previous attempt's response was lost after the ref update), the push SHALL count as successful and be reported as such.

#### Scenario: Response lost after the ref update
- **WHEN** the proxy applied a task-branch push upstream but the connection dropped before `report-status` reached the runtime
- **THEN** the retry finds the remote ref at the pushed SHA and the runtime sends one `PushReport` with status `OK`
