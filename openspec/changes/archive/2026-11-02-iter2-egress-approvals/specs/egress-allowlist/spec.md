# Spec Delta

## MODIFIED Requirements

### Requirement: Allowlist entries
Board members SHALL list, add and remove entries at instance, project or task scope. Entries are of kind `http-host` (hostname or `*.suffix`, port default 443, 80 only explicitly; all scopes) or `cluster-service` (namespace, service, ports, optional `allowDns`; instance and project scope only). A run's effective allowlist SHALL be the union of instance entries, its project's entries and its task's entries.

#### Scenario: Instance entry for Claude
- **WHEN** a board member adds instance entry `api.anthropic.com`
- **THEN** every run of every project may connect to it

#### Scenario: Project entry
- **WHEN** entry `*.crates.io` is added to project `DF`
- **THEN** runs of `DF` may connect to `static.crates.io` and runs of other projects may not

#### Scenario: Task entry
- **WHEN** entry `pypi.org` is added to task `DF-7`
- **THEN** runs of `DF-7` may connect to `pypi.org` and runs of `DF-8` may not

#### Scenario: Task entries listed
- **WHEN** a board member opens the allowlist page of project `DF`
- **THEN** task entries are listed with their task key and can be removed
