## ADDED Requirements

### Requirement: Gateway MCP server in the session
When `Assign` carries an MCP endpoint, the runtime SHALL register it in `session/new` and `session/load` as HTTP MCP server `ext`, without headers, next to `factory`, if the agent's `initialize` response advertises `mcpCapabilities.http`. If it does not, the runtime SHALL start the session with `factory` only and emit a `harness.mcp_http_unsupported` notice.

#### Scenario: Claude Code with Context7
- **WHEN** a `claude-code` run starts with MCP endpoint `https://<gateway>:8443/mcp`
- **THEN** `session/new` lists `factory` (stdio) and `ext` (`http`, that URL, no headers), and the model sees the tool `mcp__ext__context7__get-library-docs`

#### Scenario: Resumed session
- **WHEN** run 2 of a task resumes with `session/load` and has an MCP endpoint
- **THEN** `session/load` lists `factory` and `ext`

#### Scenario: Adapter without HTTP MCP support
- **WHEN** the adapter's `initialize` response has no `mcpCapabilities.http`
- **THEN** the session starts with `factory` only and the transcript shows the `harness.mcp_http_unsupported` notice

#### Scenario: No endpoint
- **WHEN** `Assign` carries no MCP endpoint
- **THEN** `session/new` lists only `factory`

### Requirement: Gateway MCP traffic bypasses the egress proxy
The harness SHALL reach the gateway MCP endpoint directly, not through the CONNECT proxy, and SHALL trust its certificate through the instance CA variables it already receives for the inference endpoint.

#### Scenario: Direct connection
- **WHEN** the harness calls the `ext` server
- **THEN** the request goes directly to the gateway's MCP endpoint and the egress log shows no `CONNECT` for it
