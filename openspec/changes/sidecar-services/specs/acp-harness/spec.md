## ADDED Requirements

### Requirement: Sidecar MCP servers in the session
The runtime SHALL register every entry of `AcpHarness.sidecar_mcp_servers` as an HTTP MCP server (its name and URL, no headers) in `session/new` and in `session/load`, after `factory` and the gateway server `ext` (`iter4-mcp-gateway`). When the agent's `initialize` response doesn't advertise `mcpCapabilities.http`, the runtime SHALL skip them and emit one `harness.mcp_http_unsupported` notice naming every skipped server. An empty list SHALL leave the session unchanged.

#### Scenario: Playwright registered
- **WHEN** a run starts with `AcpHarness.sidecar_mcp_servers` = [`playwright`, `http://localhost:8931/mcp`] and the harness advertises HTTP MCP support
- **THEN** `session/new` lists the stdio server `factory` and the HTTP server `playwright` with URL `http://localhost:8931/mcp`

#### Scenario: With the gateway server
- **WHEN** the run also has a gateway MCP endpoint
- **THEN** `session/new` lists `factory`, `ext` and `playwright`, in this order

#### Scenario: Resumed session
- **WHEN** run 2 of `DF-7` resumes the session of run 1 with `session/load` and run 2's `AcpHarness.sidecar_mcp_servers` is [`playwright`, `http://localhost:8931/mcp`]
- **THEN** `session/load` lists `factory` and `playwright` from run 2's `Assign`

#### Scenario: Harness without HTTP MCP
- **WHEN** the harness doesn't advertise HTTP MCP support
- **THEN** only `factory` is registered and the transcript shows one `harness.mcp_http_unsupported` notice naming `playwright`

#### Scenario: No sidecar MCP servers
- **WHEN** `AcpHarness.sidecar_mcp_servers` is empty and there is no gateway endpoint
- **THEN** `session/new` lists only `factory`
