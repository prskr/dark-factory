# Spec Delta

## MODIFIED Requirements

### Requirement: Fresh workspace
If `/data/repo/.git` doesn't exist, the runtime SHALL make a blobless clone of `Assign.workspace.repo_url`, write its own `.git/config`, apply the hidden paths of `Assign.workspace` as sparse patterns, check out the task branch from `origin/<task branch>` if it exists or else from `origin/<base>`, and restore files from the WIP branch if it exists, except files under hidden paths. For a task on an existing branch, a missing `origin/<task branch>` SHALL fail prepare with a reason naming the branch instead of creating it from the base.

#### Scenario: First run
- **WHEN** run 1 of `DF-1` prepares an empty volume
- **THEN** `/data/repo` is on branch `factory/task/DF-1` at `origin/main`, cloned with `--filter=blob:none`

#### Scenario: Volume lost after earlier runs
- **WHEN** the volume is empty but `factory/task/DF-1` and `factory/wip/DF-1` exist on the remote
- **THEN** the task branch is checked out and the WIP files are restored into the working tree

#### Scenario: Hidden path in a fresh workspace
- **WHEN** run 1 of `HL-1` prepares an empty volume and the project hides `talos/**`
- **THEN** no blob below `talos/` is fetched and `/data/repo/talos` doesn't exist

#### Scenario: First run on an existing branch
- **WHEN** run 1 of `DF-13` on the existing branch `renovate/serde-1.x` prepares an empty volume
- **THEN** `/data/repo` is on branch `renovate/serde-1.x` at `origin/renovate/serde-1.x`

#### Scenario: Existing branch missing
- **WHEN** run 1 of `DF-14` on the existing branch `fix/login` prepares and `origin/fix/login` doesn't exist
- **THEN** prepare fails with a reason naming `fix/login` and no branch is pushed

### Requirement: Existing workspace
If the workspace exists, the runtime SHALL rewrite `.git/config`, re-apply the hidden paths of `Assign.workspace` as sparse patterns (removing files under newly hidden paths), fetch base and task branch, push local commits missing on the remote, fast-forward if the remote is ahead, and on divergence leave both untouched and emit a `workspace.diverged` notice. It SHALL NOT run `git clean`. Under the `with-lease` policy, a local branch whose history was rewritten while the remote still points to the commit the runtime last pushed or fetched SHALL be pushed with lease instead of counting as diverged.

#### Scenario: Unpushed commits from a killed run
- **WHEN** the previous run was killed after committing but before pushing
- **THEN** prepare pushes those commits and reports the push

#### Scenario: Diverged
- **WHEN** local and remote task branches both have commits the other lacks, and the remote moved since the runtime last pushed or fetched it
- **THEN** nothing is reset, a `workspace.diverged` notice is emitted and the run report marks the workspace diverged

#### Scenario: Hidden path removed from the policy
- **WHEN** run 2 starts and `talos/**` is no longer hidden
- **THEN** `talos/` is checked out again

#### Scenario: Unpushed rebase from a killed run
- **WHEN** the policy is `with-lease`, the previous run was killed after a rebase but before pushing, and the remote is still at the commit the runtime last pushed
- **THEN** prepare pushes the rebased branch with lease and reports the push

#### Scenario: Unpushed rebase under never
- **WHEN** the policy is `never` and the previous run was killed after a rebase but before pushing
- **THEN** nothing is pushed or reset, a `workspace.diverged` notice is emitted and the run report marks the workspace diverged

### Requirement: Push after every turn with new commits
After each turn whose HEAD differs from the last pushed commit, the runtime SHALL push the task branch with an explicit refspec and SHALL send a `PushReport` with status and commits ahead of base. A fast-forward SHALL be pushed without force. Under the `with-lease` policy, a non-fast-forward SHALL be pushed with `--force-with-lease`, expecting the remote commit the runtime itself last pushed or fetched. Under `never`, it SHALL NOT be forced.

#### Scenario: Turn with commits
- **WHEN** a turn ends with 2 new commits
- **THEN** the task branch is pushed and a `PushReport { target: TASK_BRANCH, status: OK, commits_ahead_of_base: 2 }` is sent

#### Scenario: Rebase under with-lease
- **WHEN** the policy is `with-lease`, the agent rebased the task branch onto a moved base, and the remote is still at the commit the runtime last pushed
- **THEN** the branch is pushed with lease and the report has status `OK`

#### Scenario: Remote moved
- **WHEN** the push is rejected as non-fast-forward or because the lease no longer matches
- **THEN** the report has `REJECTED_NON_FAST_FORWARD`, no further force push happens and the task branch is left as is

#### Scenario: Agent fetched on its own
- **WHEN** the policy is `with-lease`, a human pushed to the task branch, and the agent ran `git fetch` itself and then rewrote the branch without the human's commit
- **THEN** the lease expects the commit the runtime last pushed or fetched, so the push is rejected and the human's commit stays on the remote

#### Scenario: Rebase under never
- **WHEN** the policy is `never` and the agent rebased the task branch
- **THEN** the push is not forced and the report has `REJECTED_NON_FAST_FORWARD`
