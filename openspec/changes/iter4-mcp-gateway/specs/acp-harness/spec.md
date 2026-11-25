## ADDED Requirements

### Requirement: Third-party tools through the runtime relay
When `Assign` carries an MCP endpoint, the runtime SHALL register a second stdio MCP server `ext` in `session/new` and `session/load`, next to `factory`, for every harness: the command `factory-agent mcp-relay` with the endpoint URL. Without an endpoint, only `factory` SHALL be registered.

#### Scenario: Claude Code with Context7
- **WHEN** a `claude-code` run starts with MCP endpoint `https://<gateway>:8443/mcp`
- **THEN** `session/new` lists `factory` and `ext` (both stdio), and the model sees the tool `mcp__ext__context7__get-library-docs`

#### Scenario: OpenCode
- **WHEN** an `opencode` run of a project with `context7` enabled starts
- **THEN** `session/new` lists `factory` and `ext`, and the model's tool list contains the `context7` tools

#### Scenario: Resumed session
- **WHEN** run 2 of a task resumes with `session/load` and has an MCP endpoint
- **THEN** `session/load` lists `factory` and `ext`

#### Scenario: No endpoint
- **WHEN** `Assign` carries no MCP endpoint
- **THEN** `session/new` lists only `factory`

### Requirement: Relay forwards messages unchanged
`factory-agent mcp-relay` SHALL forward every MCP message from stdio to the gateway endpoint as Streamable HTTP and every answer back to stdout, without changing them. It SHALL connect directly to the gateway, not through the egress proxy, SHALL trust only the instance CA, SHALL send no credential, and SHALL open no listening socket.

#### Scenario: Call through the relay
- **WHEN** the harness calls `context7__get-library-docs` on `ext`
- **THEN** the relay posts the same JSON-RPC message to `https://<gateway>:8443/mcp` with the session id from `initialize`, and writes the gateway's answer unchanged to stdout

#### Scenario: Gateway unreachable
- **WHEN** the gateway cannot be reached for a request
- **THEN** the relay answers that request on stdout with a JSON-RPC error naming the cause, and keeps running

#### Scenario: Proxy environment
- **WHEN** the relay runs with `HTTPS_PROXY` set to the gateway's proxy port
- **THEN** its requests go directly to the MCP endpoint and the egress log shows no `CONNECT` for them
