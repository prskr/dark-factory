## ADDED Requirements

### Requirement: Hidden paths per project
Board members SHALL configure a list of hidden path globs per project, one glob per line, in the syntax of protected path globs. A glob containing `{` or `}` SHALL be rejected with a validation error naming the line. The default SHALL be an empty list. Changes SHALL be audited with the push policy.

#### Scenario: Directory hidden
- **WHEN** a board member saves `talos/**` as hidden path of project `HL`
- **THEN** the policy of `HL` lists `talos/**` and an audit entry records the change

#### Scenario: Alternation rejected
- **WHEN** a board member saves `{talos,secrets}/**`
- **THEN** the form shows a validation error for that line and nothing is saved

### Requirement: Hidden paths reach the runtime
The control plane SHALL send the project's hidden globs with every run's workspace assignment, as configured when the run starts.

#### Scenario: Policy changed between runs
- **WHEN** a board member adds `talos/**` while run 1 of `HL-3` is running
- **THEN** run 1 keeps its workspace and run 2 receives `talos/**`

### Requirement: Hidden paths are absent from the workspace
The runtime SHALL keep every tracked file matching a hidden glob out of the working tree. In a fresh workspace their content SHALL NOT be downloaded during prepare. Commits the agent makes SHALL keep those files unchanged.

#### Scenario: Fresh workspace
- **WHEN** run 1 of `HL-3` prepares an empty volume and the project hides `talos/**`
- **THEN** `/data/repo/talos` doesn't exist and every other tracked file is checked out

#### Scenario: Agent commits elsewhere
- **WHEN** the agent edits `README.md` and runs `git commit -a`
- **THEN** the commit changes only `README.md`

#### Scenario: Agent edits the sparse patterns
- **WHEN** the agent clears `.git/info/sparse-checkout` and checks out `talos/` again
- **THEN** the next prepare removes `talos/` from the working tree again, before the harness starts

#### Scenario: Wildcard in the last component
- **WHEN** the project hides `configs/*`
- **THEN** `configs/top.txt` is absent and `configs/nested/visible.txt` is checked out

### Requirement: Newly hidden paths are removed
When a reused workspace holds files under a hidden glob, the runtime SHALL remove them from the working tree during prepare, also when they have uncommitted changes or are ignored by Git, and SHALL emit a `workspace.hidden_paths_removed` notice listing the removed paths.

#### Scenario: Uncommitted edits under a newly hidden path
- **WHEN** run 2 of `HL-3` starts with `talos/**` newly hidden and `talos/cp.yaml` modified but not committed
- **THEN** `talos/` is absent from the working tree and a `workspace.hidden_paths_removed` notice names `talos/cp.yaml`

#### Scenario: Ignored file under a newly hidden path
- **WHEN** run 2 starts with `talos/**` newly hidden and a Git-ignored `talos/local.yaml` on disk
- **THEN** `talos/local.yaml` is removed and named in the notice

### Requirement: Agent is told about hidden paths
When a project has hidden globs, the system prompt of every run SHALL contain a fixed section listing them, stating that they are left out of the workspace on purpose and must not be restored or read through Git. Without hidden globs the section SHALL be absent.

#### Scenario: Prompt section
- **WHEN** a run of project `HL` with hidden glob `talos/**` starts
- **THEN** the system prompt names `talos/**` and asks the agent not to restore it

#### Scenario: No hidden paths
- **WHEN** a run of a project without hidden globs starts
- **THEN** the system prompt contains no hidden-path section
