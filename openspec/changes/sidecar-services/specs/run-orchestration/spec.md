## ADDED Requirements

### Requirement: Sidecar snapshot of a run
Provisioning a run SHALL store the project's sidecars on the run (empty for chat runs and for projects without sidecars) before the sandbox is created, and SHALL provision the sandbox from that snapshot. `Assign` and the system prompt of the run SHALL use the same snapshot.

#### Scenario: Snapshot at provisioning
- **WHEN** run 1 of `DF-7` is provisioned while project `DF` has sidecars `playwright` and `postgres`
- **THEN** the run stores both sidecars and its pod carries both

#### Scenario: Provisioned again
- **WHEN** run 1 of `DF-7` is provisioned again after a replica stopped, and a board member removed `postgres` in between
- **THEN** the pod still carries both sidecars of the stored snapshot

### Requirement: Sidecar MCP servers in Assign
`Assign.sidecar_mcp_servers` SHALL contain one entry per sidecar of the run's snapshot that has an MCP endpoint: the sidecar's name and the URL `http://localhost:<port><path>`, in the snapshot's order.

#### Scenario: Assign with a Playwright sidecar
- **WHEN** run 1 of `DF-7` connects with sidecar `playwright` (MCP on port 8931, path `/mcp`) and sidecar `postgres` (no MCP endpoint)
- **THEN** `Assign.sidecar_mcp_servers` is exactly [`playwright`, `http://localhost:8931/mcp`]
