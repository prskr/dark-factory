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

#### Scenario: Repository defines `ext`
- **WHEN** the repository's harness configuration defines its own MCP server `ext`
- **THEN** the precedence is the harness's, the same as for a repository server named `factory`; this is accepted and checked by spike 1.2

### Requirement: Relay forwards messages unchanged
`factory-agent mcp-relay` SHALL forward every MCP message from stdio to the gateway endpoint as Streamable HTTP and every answer back to stdout with the same content. It SHALL connect directly to the gateway, not through the egress proxy, SHALL trust only the instance CA, SHALL send no credential, and SHALL open no listening socket.

#### Scenario: Call through the relay
- **WHEN** the harness calls `context7__get-library-docs` on `ext`
- **THEN** the relay posts the same JSON-RPC message to `https://<gateway>:8443/mcp` with the session id from `initialize`, and writes the gateway's answer to stdout

#### Scenario: Gateway unreachable
- **WHEN** the gateway cannot be reached for a request
- **THEN** the relay answers that request on stdout with a JSON-RPC error naming the cause, and keeps running

### Requirement: Relay framing and errors
The relay SHALL write exactly one compact JSON line per message to stdout, never interleaving lines. A gateway answer with an HTTP error status to a request SHALL become a JSON-RPC error for that request id. For a notification the relay SHALL write nothing to stdout, whatever the answer; an HTTP error status for a notification SHALL be logged on stderr.

#### Scenario: Pretty-printed answer
- **WHEN** the gateway's answer body contains newlines
- **THEN** the relay writes it as one line without them

#### Scenario: Gateway answers 403
- **WHEN** the gateway answers a `tools/call` with 403
- **THEN** the relay writes a JSON-RPC error for that request id naming status 403

#### Scenario: Notification
- **WHEN** the harness sends `notifications/initialized` and the gateway answers 202
- **THEN** the relay writes nothing to stdout

#### Scenario: Notification refused
- **WHEN** the harness sends `notifications/cancelled` and the gateway answers 400
- **THEN** the relay writes nothing to stdout and logs status 400 on stderr

### Requirement: Relay concurrency and lifetime
The relay SHALL handle requests concurrently, so a slow call does not delay other requests. It SHALL exit with status 0 when stdin ends, and SHALL exit non-zero with a message on stderr when its CA file cannot be read.

#### Scenario: Slow call and ping
- **WHEN** a call waits 100 s for the gateway and the harness sends `ping`
- **THEN** the `ping` answer is written at once

#### Scenario: Harness ends
- **WHEN** the harness closes the relay's stdin
- **THEN** the relay exits with status 0

#### Scenario: CA missing
- **WHEN** the relay starts with a `--ca` path that does not exist
- **THEN** it exits non-zero and names the path on stderr

#### Scenario: Proxy environment
- **WHEN** the relay runs with `HTTPS_PROXY` set to the gateway's proxy port
- **THEN** its requests go directly to the MCP endpoint and the egress log shows no `CONNECT` for them
