## ADDED Requirements

### Requirement: Sidecar MCP servers in the session
The runtime SHALL register every entry of `Assign.sidecar_mcp_servers` as an HTTP MCP server (its name and URL, no headers) after the factory MCP server in `session/new` and in `session/load`. When the harness's `initialize` response doesn't advertise HTTP MCP support, the runtime SHALL register only the factory MCP server and SHALL emit a transcript warning `harness.sidecar_mcp_unsupported` naming the skipped servers. An empty list SHALL leave the session unchanged.

#### Scenario: Playwright registered
- **WHEN** a run starts with `Assign.sidecar_mcp_servers` = [`playwright`, `http://localhost:8931/mcp`] and the harness advertises HTTP MCP support
- **THEN** `session/new` lists the stdio server `factory` and the HTTP server `playwright` with URL `http://localhost:8931/mcp`

#### Scenario: Resumed session
- **WHEN** the same run resumes a session with `session/load`
- **THEN** `session/load` lists the same two servers

#### Scenario: Harness without HTTP MCP
- **WHEN** the harness doesn't advertise HTTP MCP support
- **THEN** only `factory` is registered and the transcript shows a warning naming `playwright`

#### Scenario: No sidecar MCP servers
- **WHEN** `Assign.sidecar_mcp_servers` is empty
- **THEN** `session/new` lists only `factory`
