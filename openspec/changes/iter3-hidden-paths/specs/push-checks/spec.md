## ADDED Requirements

### Requirement: Hidden paths
A checked commit that adds, modifies, deletes or renames a path matching one of the project's hidden globs SHALL be a `df:hidden-path` finding with mode `reject`, regardless of the protected path mode. It SHALL never raise a request. The check SHALL run for task-branch and WIP-branch pushes.

#### Scenario: Hidden file deleted
- **WHEN** the project hides `talos/**` and a commit on the task branch runs `git rm --cached --sparse talos/cp.yaml`
- **THEN** the push is rejected with `df:hidden-path` naming `talos/cp.yaml` and no request exists

#### Scenario: Hidden file in a WIP snapshot
- **WHEN** a WIP push contains a change under `talos/`
- **THEN** the WIP push is rejected with `df:hidden-path`

#### Scenario: Approval mode does not apply
- **WHEN** the project's protected path mode is `approval` and a commit adds `talos/new.yaml`
- **THEN** the push is rejected with `df:hidden-path` and no `push-check-exception` request is raised

## MODIFIED Requirements

### Requirement: WIP snapshots
Pushes to the WIP branch SHALL get only the secret scan, the size limit and the hidden path check, all with mode `reject`, and SHALL never raise requests.

#### Scenario: Secret in uncommitted work
- **WHEN** the WIP snapshot contains a private key in an untracked, non-ignored file
- **THEN** the WIP push is rejected with `df:secret-scan` and no request exists

### Requirement: Per-project push policy
Board members SHALL configure per project: protected path globs (default `.forgejo/workflows/**`, `.github/workflows/**`, `.gitea/workflows/**`, `CODEOWNERS`), protected path mode (default `approval`), hidden path globs (default none), secret scan mode (default `approval`), blob size limit (default 5 MiB), blob size mode (default `approval`) and linear history (default off). Changes SHALL be audited and reach gateways within a second.

#### Scenario: Policy change applies to the next push
- **WHEN** a board member adds `deploy/**` as protected path to project `DF`
- **THEN** the next push of a `DF` task that changes `deploy/values.yaml` produces a `df:protected-path` finding

#### Scenario: Hidden path applies to the next push
- **WHEN** a board member adds `talos/**` as hidden path to project `HL`
- **THEN** the next push of an `HL` task that changes `talos/cp.yaml` is rejected with `df:hidden-path`
