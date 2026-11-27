## ADDED Requirements

### Requirement: Sidecar snapshot of a run
Creating a run SHALL store the project's sidecars on the run in the same transaction that creates the run row (an empty list for chat runs and for projects without sidecars). Every provisioning attempt of the run SHALL use that snapshot, and `Assign` and the system prompt of the run SHALL use the same snapshot.

#### Scenario: Snapshot with the run
- **WHEN** run 1 of `DF-7` is created while project `DF` has sidecars `playwright` and `postgres`
- **THEN** the run row stores both sidecars and the run's pod carries both

#### Scenario: Provisioned again
- **WHEN** run 1 of `DF-7` is provisioned again after a replica stopped, and a board member removed `postgres` in between
- **THEN** the pod still carries both sidecars of the stored snapshot

#### Scenario: Interrupted before provisioning
- **WHEN** a replica stops after creating run 1 of `DF-7` and before creating its sandbox
- **THEN** the next tick provisions the run with the two sidecars stored at creation

### Requirement: Sidecar MCP servers in the harness settings
`AcpHarness.sidecar_mcp_servers` SHALL contain one entry per sidecar of the run's snapshot that has an MCP endpoint: the sidecar's name and the URL `http://localhost:<port><path>`, in the snapshot's order.

#### Scenario: Assign with a Playwright sidecar
- **WHEN** run 1 of `DF-7` connects with sidecar `playwright` (MCP on port 8931, path `/mcp`) and sidecar `postgres` (no MCP endpoint)
- **THEN** `AcpHarness.sidecar_mcp_servers` is exactly [`playwright`, `http://localhost:8931/mcp`]
