# Spec Delta

## ADDED Requirements

### Requirement: Allow from the log
Each denied event with reason `not-allowlisted` and each denied-hosts summary row SHALL offer an action to allow the host and port for the event's task, its project or the instance. The action SHALL create the entry like the allowlist form does (validated, audited, announced).

#### Scenario: Allow for the project
- **WHEN** a board member chooses "allow for project" on a denied `files.pythonhosted.org:443` event of `DF-7`
- **THEN** a project entry for `DF` exists and the next attempt of `DF-7` succeeds

#### Scenario: Wildcard not offered
- **WHEN** the denied host is `static.crates.io`
- **THEN** the action allows exactly `static.crates.io`, not `*.crates.io`
