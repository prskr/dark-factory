# Spec Delta

## MODIFIED Requirements

### Requirement: Assign content
`Assign` SHALL contain the task reference, the workspace (project repo URL, base branch, task and WIP branch, the instance's commit identity), the persona's system prompt and ACP harness (adapter command, non-secret env, model, settings), the factory tools v1, budgets from instance configuration and the continuation. The adapter command SHALL be the project's adapter command if it sets one, otherwise `harness.claudeCode.adapterCommand` of the instance.

#### Scenario: Assign for DF-1
- **WHEN** run 1 of `DF-1` connects
- **THEN** `Assign.workspace.task_branch` is `factory/task/DF-1` and `Assign.tools` contains exactly the four factory tools v1

#### Scenario: Default adapter command
- **WHEN** a run of a project without an adapter command connects
- **THEN** `Assign.harness.acp.command` is the instance's `harness.claudeCode.adapterCommand`

#### Scenario: Project adapter command
- **WHEN** a run of a project with adapter command `/opt/harness/bin/fake-acp-agent` connects
- **THEN** `Assign.harness.acp.command` is `["/opt/harness/bin/fake-acp-agent"]`
