# Spec Delta

## MODIFIED Requirements

### Requirement: Assign content
`Assign` SHALL contain the task reference, the workspace (the task's Git proxy URL `https://<gateway host>:<git port>/git/<KEY>.git` as repo URL, base branch, task and WIP branch, the instance's commit identity), the persona's system prompt and ACP harness (adapter command, non-secret env, model, settings), the factory tools, budgets from instance configuration and the continuation. The adapter command SHALL be the project's adapter command if it sets one, otherwise `harness.claudeCode.adapterCommand` of the instance.

#### Scenario: Assign for DF-1
- **WHEN** run 1 of `DF-1` connects
- **THEN** `Assign.workspace.task_branch` is `factory/task/DF-1` and `Assign.tools` contains exactly the factory tools of the current tool set

#### Scenario: Default adapter command
- **WHEN** a run of a project without an adapter command connects
- **THEN** `Assign.harness.acp.command` is the instance's `harness.claudeCode.adapterCommand`

#### Scenario: Project adapter command
- **WHEN** a run of a project with adapter command `/opt/harness/bin/fake-acp-agent` connects
- **THEN** `Assign.harness.acp.command` is `["/opt/harness/bin/fake-acp-agent"]`

#### Scenario: Repository through the proxy
- **WHEN** run 1 of `DF-1` connects in an instance whose gateway Service is `df-gateway.df.svc` with Git port 8443
- **THEN** `Assign.workspace.repo_url` is `https://df-gateway.df.svc:8443/git/DF-1.git` and contains no credentials
