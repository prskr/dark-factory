# Spec Delta

## ADDED Requirements

### Requirement: Allow from the log
Each denied event with reason `not-allowlisted` SHALL offer an action to allow its host and port for the event's task, its project or the instance. Each denied-hosts summary row SHALL offer task scope only if it covers exactly one task, project scope only if it covers exactly one project, and always instance scope. Every action SHALL create the entry like the allowlist form does (validated, audited, announced).

#### Scenario: Allow for the project
- **WHEN** a board member chooses "allow for project" on a denied `files.pythonhosted.org:443` event of `DF-7`
- **THEN** a project entry for `DF` exists and the next attempt of `DF-7` succeeds

#### Scenario: Summary row across projects
- **WHEN** the denied-hosts summary row for `statsig.anthropic.com:443` covers tasks of projects `DF` and `OPS`
- **THEN** the row offers only instance scope

#### Scenario: Summary row of one project
- **WHEN** the summary row covers tasks `DF-7` and `DF-8` of project `DF`
- **THEN** the row offers project and instance scope, but not task scope

#### Scenario: Wildcard not offered
- **WHEN** the denied host is `static.crates.io`
- **THEN** the action allows exactly `static.crates.io`, not `*.crates.io`
