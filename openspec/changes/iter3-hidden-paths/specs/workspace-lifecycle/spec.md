## MODIFIED Requirements

### Requirement: Fresh workspace
If `/data/repo/.git` doesn't exist, the runtime SHALL make a blobless clone of `Assign.workspace.repo_url`, write its own `.git/config`, apply the hidden paths of `Assign.workspace` as sparse patterns, check out the task branch from `origin/<task branch>` if it exists or else from `origin/<base>`, and restore files from the WIP branch if it exists, except files under hidden paths.

#### Scenario: First run
- **WHEN** run 1 of `DF-1` prepares an empty volume
- **THEN** `/data/repo` is on branch `factory/task/DF-1` at `origin/main`, cloned with `--filter=blob:none`

#### Scenario: Volume lost after earlier runs
- **WHEN** the volume is empty but `factory/task/DF-1` and `factory/wip/DF-1` exist on the remote
- **THEN** the task branch is checked out and the WIP files are restored into the working tree

#### Scenario: Hidden path in a fresh workspace
- **WHEN** run 1 of `HL-1` prepares an empty volume and the project hides `talos/**`
- **THEN** no blob below `talos/` is fetched and `/data/repo/talos` doesn't exist

### Requirement: Existing workspace
If the workspace exists, the runtime SHALL rewrite `.git/config`, re-apply the hidden paths of `Assign.workspace` as sparse patterns (removing files under newly hidden paths), fetch base and task branch, push local commits missing on the remote, fast-forward if the remote is ahead, and on divergence leave both untouched and emit a `workspace.diverged` notice. It SHALL NOT run `git clean`.

#### Scenario: Unpushed commits from a killed run
- **WHEN** the previous run was killed after committing but before pushing
- **THEN** prepare pushes those commits and reports the push

#### Scenario: Diverged
- **WHEN** local and remote task branches both have commits the other lacks
- **THEN** nothing is reset, a `workspace.diverged` notice is emitted and the run report marks the workspace diverged

#### Scenario: Hidden path removed from the policy
- **WHEN** run 2 starts and `talos/**` is no longer hidden
- **THEN** `talos/` is checked out again

### Requirement: Finalize
At the end of every run, including cancelled and failed ones where possible, the runtime SHALL stop the harness (graceful, then kill after a timeout), rewrite `.git/config`, re-apply the hidden paths as sparse patterns, push the task branch, write a WIP snapshot without touching the agent's index, HEAD or branches, and send `RunReport` with outcome, branch SHAs, push state, divergence, workspace size and harness session id. The WIP snapshot SHALL leave every path under a hidden glob as it is in HEAD.

#### Scenario: Uncommitted work
- **WHEN** the run ends with modified, uncommitted files
- **THEN** a commit containing them is force-pushed to `factory/wip/DF-1`, and `git status` in the workspace is unchanged

#### Scenario: Clean worktree
- **WHEN** the run ends with no uncommitted changes
- **THEN** the remote WIP branch is deleted if it exists and a `PushReport` with `DELETED` is sent

#### Scenario: Clean worktree with hidden paths
- **WHEN** the project hides `talos/**` and the run ends with no uncommitted changes
- **THEN** the WIP tree equals HEAD's tree and the remote WIP branch is deleted

### Requirement: Push rejected by a check
When the Git proxy rejects a push with a `df:` reason, the runtime SHALL send a `PushReport` with `REJECTED_SECRET_DETECTED` for `df:secret-scan`; `REJECTED_BY_CHECK` for `df:identity`, `df:protected-path`, `df:hidden-path`, `df:blob-too-large`, `df:merge-commit` and `df:push-too-large`; `REJECTED_NON_FAST_FORWARD` for `df:non-fast-forward`; and `FAILED` for `df:audit-unavailable` and any other reason. The detail SHALL be the reason text, and the rejected commits SHALL count as not pushed.

#### Scenario: Rejected after a turn
- **WHEN** the push after a turn is rejected with `df:protected-path`
- **THEN** the runtime sends `PushReport { status: REJECTED_BY_CHECK }` with the reason as detail

#### Scenario: Retry after the next turn
- **WHEN** a push was rejected and the next turn produces no new commits
- **THEN** the runtime pushes the task branch again after that turn

#### Scenario: Audit unavailable
- **WHEN** the push is rejected with `df:audit-unavailable`
- **THEN** the runtime sends `PushReport { status: FAILED }` with the reason as detail and pushes again after the next turn

#### Scenario: Ref rule rejection
- **WHEN** the push is rejected with `df:non-fast-forward`
- **THEN** the runtime sends `PushReport { status: REJECTED_NON_FAST_FORWARD }` and never force-pushes

#### Scenario: Hidden path rejection
- **WHEN** the push after a turn is rejected with `df:hidden-path`
- **THEN** the runtime sends `PushReport { status: REJECTED_BY_CHECK }` with the reason as detail
