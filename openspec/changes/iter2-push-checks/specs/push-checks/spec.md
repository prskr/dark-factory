# Spec Delta

## Purpose

Checks every pushed commit at the Git proxy before it reaches the forge: no secrets, no foreign identities, no edits to protected paths and no oversized blobs unless a human approved the exception (`design/agent-run.md` §11).

## ADDED Requirements

### Requirement: Checked commits
Checks SHALL run before forwarding, on the commits a push adds (reachable from a pushed new SHA, not reachable from any upstream ref). Commits already on the forge SHALL NOT be checked again.

#### Scenario: Rebased onto a new base
- **WHEN** the task branch is rebased onto a `main` that contains human commits and pushed
- **THEN** only the agent's rebased commits are checked

### Requirement: Secret scan
The proxy SHALL scan the content of blobs added by checked commits with the secret rule set, without contacting any service to validate findings. A finding SHALL record rule, path, line and a masked preview, and SHALL NOT store or log the matched value.

#### Scenario: API key committed
- **WHEN** a pushed commit adds `config.py` containing a GitHub token
- **THEN** the push is rejected with reason `df:secret-scan` naming the rule, path and line, and the record contains only the masked preview

### Requirement: Commit identity
Each checked commit's author and committer email SHALL equal the instance's commit identity; otherwise the push SHALL be rejected with `df:identity`. This check SHALL NOT be configurable.

#### Scenario: Commit as a human
- **WHEN** a pushed commit has author `peter@example.org`
- **THEN** the push is rejected with `df:identity`

### Requirement: Protected paths
A checked commit that adds, modifies, deletes or renames a path matching a protected glob SHALL be a `df:protected-path` finding. `.dark-factory/**` SHALL always be protected with mode `reject`; project globs SHALL use the project's configured mode.

#### Scenario: Workflow edited
- **WHEN** a commit changes `.forgejo/workflows/ci.yml` and the project protects `.forgejo/workflows/**` with mode `approval`
- **THEN** the push is rejected with `df:protected-path` and a `push-check-exception` request is raised

#### Scenario: Repository configuration edited
- **WHEN** a commit changes `.dark-factory/config.toml`
- **THEN** the push is rejected with `df:protected-path` and no request is raised

### Requirement: Blob size limit
A blob added by a checked commit that is larger than the project's limit (default 5 MiB) SHALL be a `df:blob-too-large` finding.

#### Scenario: Large binary
- **WHEN** a commit adds an 8 MiB `data.bin` with the default limit
- **THEN** the push is rejected with `df:blob-too-large` naming the path and size

### Requirement: Linear history
When the project enables linear history, a checked commit with more than one parent SHALL cause a `df:merge-commit` rejection.

#### Scenario: Merge commit
- **WHEN** linear history is enabled and the agent pushes a merge of `main` into the task branch
- **THEN** the push is rejected with `df:merge-commit`

### Requirement: WIP snapshots
Pushes to the WIP branch SHALL get only the secret scan and the size limit, both with mode `reject`, and SHALL never raise requests.

#### Scenario: Secret in uncommitted work
- **WHEN** the WIP snapshot contains a private key in an untracked, non-ignored file
- **THEN** the WIP push is rejected with `df:secret-scan` and no request exists

### Requirement: Per-project push policy
Board members SHALL configure per project: protected path globs (default `.forgejo/workflows/**`, `.github/workflows/**`, `.gitea/workflows/**`, `CODEOWNERS`), mode of protected paths, secret scan and size limit (default `approval`), size limit and linear history (default off). Changes SHALL be audited and reach gateways within a second.

#### Scenario: Policy change applies to the next push
- **WHEN** a board member adds `deploy/**` as protected path to project `DF`
- **THEN** the next push of a `DF` task that changes `deploy/values.yaml` produces a `df:protected-path` finding

### Requirement: Exception requests
A task-branch push rejected only by approval-mode findings without a matching exception SHALL raise one `push-check-exception` request on the task listing the findings (check, path, line, masked preview). The merge fingerprint SHALL be the set of finding fingerprints.

#### Scenario: Mixed findings
- **WHEN** a push has an identity violation and a protected-path finding in approval mode
- **THEN** the push is rejected and no request is raised

### Requirement: Approved exceptions
Approving a `push-check-exception` request SHALL create task-scoped exceptions for exactly its findings (secret: rule + blob + line; protected path: path; size: path + blob). A later push of the task SHALL pass findings with a matching exception.

#### Scenario: Exception approved
- **WHEN** the request for `.forgejo/workflows/ci.yml` is approved and the runtime pushes the same commits again
- **THEN** the push is forwarded

#### Scenario: Exception doesn't cover new changes
- **WHEN** an exception exists for a secret finding and the agent commits a different secret
- **THEN** the push is rejected again

### Requirement: Obsolete exception requests
When a task-branch push of a task is accepted, pending `push-check-exception` requests of that task SHALL be closed with reason `push-accepted`.

#### Scenario: Agent fixed the commit
- **WHEN** the agent removes the secret, rewrites its unpushed commits and the push is accepted
- **THEN** the pending exception request is `closed` with reason `push-accepted`

### Requirement: Rejection feedback to the agent
A task-branch push rejected by checks SHALL queue a system message (reason `push.rejected`) on the task with the masked findings and whether an approval was requested, unless such a message is still undelivered or 3 such messages were queued since the last accepted push.

#### Scenario: Agent learns about the rejection
- **WHEN** the runtime's push after a turn is rejected with `df:secret-scan`
- **THEN** a `push.rejected` message listing the rule, path, line and masked preview is queued on the task

#### Scenario: Rejection loop
- **WHEN** a task's pushes were rejected 3 times in a row with feedback each time
- **THEN** a fourth rejection queues no message
