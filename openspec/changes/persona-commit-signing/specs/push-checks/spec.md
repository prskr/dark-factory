# Spec Delta

## ADDED Requirements

### Requirement: Commit signature
Each checked task-branch commit SHALL carry an SSH signature (namespace `git`) that verifies against the current public signing key of the pushing run's persona, as carried in the run's Git route; otherwise the push SHALL be rejected with `df:signature`. This check SHALL NOT be configurable and SHALL NOT apply to pushes to the run's WIP branch (requirement "WIP exemption of the signature check"). The rejection detail SHALL say whether the signature was missing, made by another key, or invalid.

#### Scenario: Unsigned commit
- **WHEN** the agent commits with `git -c commit.gpgSign=false commit` and the runtime pushes
- **THEN** the push is rejected with `df:signature` and detail "missing signature"

#### Scenario: Signed by the persona
- **WHEN** a run of `linus` pushes commits signed through the shim
- **THEN** they pass the signature check

#### Scenario: Signed with a rotated key
- **WHEN** `linus`'s key is rotated after a commit was signed and before it was pushed
- **THEN** the push is rejected with `df:signature` and detail "signed by another key"

#### Scenario: Re-sign hint
- **WHEN** a task-branch push is rejected with `df:signature`
- **THEN** the `push.rejected` message tells the agent to re-sign the listed commits, for example with `git rebase --exec 'git commit --amend --no-edit' <base>`

#### Scenario: Commits already on the forge
- **WHEN** a task started from an existing branch pushes on top of unsigned commits that are already on the forge
- **THEN** only the task's new commits are checked for a signature

### Requirement: WIP exemption of the signature check
The gateway SHALL exempt a push from `df:signature` only when its target ref is the WIP branch of the run's Git route, never based on commit content. Commits that are only on the WIP branch SHALL NOT count as already on the forge, so a task-branch push checks their signatures.

#### Scenario: WIP snapshot push
- **WHEN** the runtime pushes an unsigned WIP snapshot to `refs/heads/factory/wip/DF-7`
- **THEN** the push is not rejected with `df:signature`

#### Scenario: Unsigned commit from the WIP branch
- **WHEN** an unsigned commit is on `refs/heads/factory/wip/DF-7` and the task branch is then pushed pointing at it
- **THEN** the task-branch push is rejected with `df:signature` and detail "missing signature"
