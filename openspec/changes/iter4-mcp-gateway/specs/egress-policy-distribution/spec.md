## ADDED Requirements

### Requirement: MCP servers in the snapshot
The policy snapshot SHALL carry every MCP server enabled for at least one active run once (id, name, upstream URL, credential reference and scheme, "Private network" switch, a configuration version), and each sandbox entry SHALL list the ids of the servers enabled for its run's project. The snapshot SHALL carry the referenced credentials once each. Gateways SHALL keep them in memory only and never log them.

#### Scenario: Run with an MCP server
- **WHEN** a gateway receives a snapshot while run R of project `DF` is active and `DF` has `context7` enabled
- **THEN** the snapshot carries `context7` with its URL, scheme, switch and credential, and R's entry lists `context7`

#### Scenario: Server not enabled for any active run
- **WHEN** server `billing` is registered but no project with an active run enables it
- **THEN** the snapshot carries neither `billing` nor its credential

#### Scenario: Unresolvable MCP credential
- **WHEN** the Secret of a server's credential cannot be read
- **THEN** the snapshot carries the server without that credential and the other entries are unaffected

### Requirement: MCP changes propagate within a second
Editing or deleting a server and changing a project's enabled servers SHALL publish a policy notification, so that every connected gateway pod has the change within about a second, regardless of the control plane replica.

#### Scenario: Project disables a server on replica A, gateway on replica B
- **WHEN** an admin disables `context7` for `DF` through replica A
- **THEN** a gateway pod connected to replica B rejects calls of `DF` runs to `context7__` tools within one second

#### Scenario: MCP credential rotated
- **WHEN** the Kubernetes Secret behind a server's credential changes
- **THEN** gateways receive the new value with the next snapshot, within one minute
