# Spec Delta

## ADDED Requirements

### Requirement: Allow from the log
Each denied event with reason `not-allowlisted` SHALL offer an action to allow its host and port for the event's task, its project or the instance, in the action slot and menu of its events table row. Every action SHALL create the entry like the allowlist form does (validated, audited, announced).

#### Scenario: Allow for the project
- **WHEN** a board member chooses "allow for project" on a denied `files.pythonhosted.org:443` event of `DF-7`
- **THEN** a project entry for `DF` exists and the next attempt of `DF-7` succeeds

#### Scenario: Action on the table row
- **WHEN** a denied `files.pythonhosted.org:443` event of `DF-7` is listed
- **THEN** its row offers task, project and instance scope

#### Scenario: Wildcard not offered
- **WHEN** the denied host is `static.crates.io`
- **THEN** the action allows exactly `static.crates.io`, not `*.crates.io`

### Requirement: Allow from a graph node
A blocked graph node SHALL offer the action of "Allow from the log" for its host and port. It SHALL offer task scope only if all its denied edges belong to one task, project scope only if they all belong to one project, and always instance scope.

#### Scenario: Graph node of one task
- **WHEN** the only denied edges to the blocked node `files.pythonhosted.org:443` come from runs of `DF-7`
- **THEN** the node offers task, project and instance scope

#### Scenario: Graph node across tasks of one project
- **WHEN** the blocked node `statsig.anthropic.com:443` has denied edges from `DF-7` and `DF-8` of project `DF`
- **THEN** the node offers project and instance scope, but not task scope

#### Scenario: Graph node across projects
- **WHEN** the blocked node `statsig.anthropic.com:443` has denied edges from tasks of projects `DF` and `OPS`
- **THEN** the node offers only instance scope
