# Spec Delta

## MODIFIED Requirements

### Requirement: Allowed ref updates
A push SHALL be forwarded only if every ref update targets the task's stored branch as a create or update, or `refs/heads/factory/wip/<KEY>` as any create, update or delete. An update of the task branch SHALL be a fast-forward unless the project's branch rewrite policy is `with-lease`. Deleting the task branch SHALL never be allowed. Otherwise the whole push SHALL be rejected without contacting upstream.

#### Scenario: Push to main
- **WHEN** the sandbox of `DF-7` pushes `refs/heads/main`
- **THEN** git reports `remote rejected` for `main` with reason `df:ref-not-allowed` and the upstream `main` is unchanged

#### Scenario: Force push of the task branch
- **WHEN** the project's policy is `never` and a push updates `factory/task/DF-7` to a commit that doesn't contain the current remote commit
- **THEN** the push is rejected with reason `df:non-fast-forward`

#### Scenario: Rebase pushed with lease
- **WHEN** the project's policy is `with-lease` and a push updates `factory/task/DF-7` from the current remote commit to a rebased commit that doesn't contain it
- **THEN** the push is forwarded and succeeds

#### Scenario: Lease no longer matches
- **WHEN** the project's policy is `with-lease` and a non-fast-forward push of `factory/task/DF-7` names an old commit that is no longer the remote's
- **THEN** upstream rejects the update and the remote branch is unchanged

#### Scenario: Task branch deletion
- **WHEN** a push deletes `factory/task/DF-7`
- **THEN** the push is rejected with reason `df:ref-not-allowed`

#### Scenario: Existing branch of the task
- **WHEN** `DF-13` holds `renovate/serde-1.x` and its sandbox pushes a fast-forward of `renovate/serde-1.x`
- **THEN** the push is forwarded and succeeds

#### Scenario: Branch pattern of the key
- **WHEN** `DF-13` holds `renovate/serde-1.x` and its sandbox pushes `factory/task/DF-13`
- **THEN** the push is rejected with reason `df:ref-not-allowed`

#### Scenario: WIP snapshot
- **WHEN** the runtime force-pushes `factory/wip/DF-7`
- **THEN** the push is forwarded and succeeds

#### Scenario: Mixed push
- **WHEN** one push updates `factory/task/DF-7` and `refs/tags/v1`
- **THEN** both ref updates are rejected and nothing reaches upstream
