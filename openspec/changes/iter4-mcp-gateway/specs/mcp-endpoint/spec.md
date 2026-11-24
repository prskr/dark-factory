## Purpose

The gateway's MCP endpoint for third-party tools: it identifies the calling run, offers the run's pinned tool set of all enabled upstream MCP servers as one MCP server, forwards permitted calls with the upstream credential and reports one record per call, so that no upstream credential is needed in a sandbox.

## ADDED Requirements

### Requirement: MCP endpoint
The gateway SHALL serve one MCP server (Streamable HTTP transport) at `https://<gateway host>:<git port>/mcp` with the certificate of the instance CA, on the same listener as the Git proxy and the inference endpoint. It SHALL answer `POST` with `application/json`, answer `GET` with 405 because it sends no server-initiated messages, and answer `DELETE` with 204.

#### Scenario: Initialize
- **WHEN** the harness of run R sends an MCP `initialize` request to `POST https://<gateway>:8443/mcp`
- **THEN** the gateway answers with server capabilities `tools` only (`listChanged: false`) and an `Mcp-Session-Id` bound to R

#### Scenario: Server stream requested
- **WHEN** a client sends `GET /mcp`
- **THEN** the gateway answers 405

#### Scenario: Harness ends its session
- **WHEN** the harness sends `DELETE /mcp` with its session id
- **THEN** the gateway answers 204 and contacts no upstream

### Requirement: Caller identification
The endpoint SHALL identify the run by source pod IP from the latest policy snapshot. It SHALL answer 403 for unknown sources and 503 until the first snapshot arrived, and SHALL contact no upstream in these cases. A known sandbox SHALL be served even when its project has no enabled server; its tool list is then empty.

#### Scenario: Unknown pod
- **WHEN** a pod that isn't an active sandbox sends `POST /mcp`
- **THEN** the gateway answers 403 and contacts no upstream

#### Scenario: Gateway just started
- **WHEN** a request arrives before the gateway received its first snapshot
- **THEN** the gateway answers 503

#### Scenario: Last server disabled
- **WHEN** the only server of project `DF` is disabled while a `DF` run has a session
- **THEN** `tools/list` returns no tools and a call is rejected with a `rejected` record

### Requirement: Sessions bound to the run
An `Mcp-Session-Id` SHALL name the run it was issued to and SHALL be accepted by every gateway pod for requests from that run's pod. A request other than `initialize` without a session id SHALL get 400. A malformed session id, or one issued to another run, SHALL get 404.

#### Scenario: Session of another run
- **WHEN** run S sends a request with a session id issued to run R
- **THEN** the gateway answers 404 and contacts no upstream

#### Scenario: Missing session id
- **WHEN** the harness sends `tools/list` without `Mcp-Session-Id`
- **THEN** the gateway answers 400

#### Scenario: Request reaches another gateway pod
- **WHEN** run R initialized its session on gateway pod A and its next request reaches pod B
- **THEN** pod B serves the request

### Requirement: Protocol version
The endpoint SHALL answer `initialize` with the protocol version the client requested if it supports it, otherwise with the latest version it supports. A request with an unsupported `MCP-Protocol-Version` header SHALL get 400. JSON-RPC batches SHALL get 400.

#### Scenario: Unsupported version header
- **WHEN** the harness sends `tools/list` with `MCP-Protocol-Version: 1999-01-01`
- **THEN** the gateway answers 400

#### Scenario: Batch
- **WHEN** the harness sends a JSON array of two requests
- **THEN** the gateway answers 400 and contacts no upstream

### Requirement: Only tools
The endpoint SHALL answer `initialize`, `ping`, `tools/list` and `tools/call`, accept the `notifications/initialized` and `notifications/cancelled` notifications, and answer every other method (resources, prompts, completion, logging) with JSON-RPC error `-32601` (method not found).

#### Scenario: Resources requested
- **WHEN** the harness sends `resources/list`
- **THEN** the gateway answers with error `-32601` and contacts no upstream

### Requirement: Aggregated tool list
`tools/list` SHALL return the run's tool set from the snapshot in one page: each pinned tool with name `<server>__<tool>` and only its pinned `title`, `description`, `inputSchema`, `outputSchema` and `annotations`, without contacting any upstream.

#### Scenario: Two servers
- **WHEN** project `DF` has `context7` (pinned `get-library-docs`) and `forgejo` (pinned `list_issues`) enabled and a `DF` run lists tools
- **THEN** it receives exactly `context7__get-library-docs` and `forgejo__list_issues` with the pinned definitions

#### Scenario: Extra fields not served
- **WHEN** the fetched definition of a pinned tool had a `_meta` and an `icons` field
- **THEN** the listed tool has neither

### Requirement: Tool set enforcement
For `tools/call` the endpoint SHALL check the name against the run's current tool set. A name outside the set SHALL get a tool result with `isError: true` and the text `tool <name> is not available in this run`, SHALL be recorded with outcome `rejected`, and SHALL NOT contact any upstream.

#### Scenario: Unlisted tool called directly
- **WHEN** the harness calls `forgejo__delete_repo`, which the upstream offers but which is not pinned
- **THEN** the call fails with `isError: true`, no request reaches the `forgejo` upstream, and a call record with outcome `rejected` is reported

#### Scenario: Server disabled after listing
- **WHEN** the harness listed `context7__get-library-docs`, then `context7` is disabled for the project, then the harness calls it
- **THEN** the call is rejected

### Requirement: Calls forwarded unchanged
A permitted call SHALL be sent to its server's upstream as `tools/call` with the original tool name and the arguments unchanged. The upstream's result (content, structured content, `isError`) SHALL be returned unchanged; an upstream JSON-RPC error SHALL be returned as a tool result with `isError: true` and the error message.

#### Scenario: Successful call
- **WHEN** the harness calls `context7__get-library-docs` with `{"libraryId": "/tokio-rs/tokio"}`
- **THEN** the `context7` upstream receives `tools/call` for `get-library-docs` with the same arguments, and the harness receives its result

### Requirement: Upstream failures
When the upstream cannot be connected to, the TLS handshake fails, or it answers with an HTTP error status, a non-MCP body or a redirect, the call SHALL get `isError: true` with a fixed message naming the cause (never the credential), outcome `upstream-error`. Redirects SHALL NOT be followed.

#### Scenario: Upstream answers 401
- **WHEN** the upstream answers a call with 401
- **THEN** the harness receives `isError: true` with "upstream refused the credential (401)" and the record has outcome `upstream-error`

#### Scenario: Redirect to an internal address
- **WHEN** the upstream answers a call with `307` and `Location: http://10.0.0.5/`
- **THEN** no request is sent to `10.0.0.5` and the call fails with outcome `upstream-error`

#### Scenario: Upstream down
- **WHEN** the upstream host refuses the connection
- **THEN** the call fails with outcome `upstream-error`

### Requirement: Credential injection
The gateway SHALL build every upstream request itself and SHALL forward no header of the sandbox's request. It SHALL inject the server's credential as defined by its scheme, and SHALL never return the credential or upstream response headers to the client, or write the credential to logs.

#### Scenario: Harness sends its own credential
- **WHEN** the harness sends `Authorization: Bearer stolen` and `CONTEXT7_API_KEY: x` to `/mcp`
- **THEN** the upstream request carries only the server's configured credential header and neither value from the harness

#### Scenario: Credential missing from the snapshot
- **WHEN** the server's credential is not in the snapshot (unresolvable Secret)
- **THEN** the call fails with `isError: true` and a fixed message, outcome `upstream-error`, and no upstream is contacted

### Requirement: Upstream sessions
The gateway SHALL be the MCP client of each upstream with one session per gateway pod, run and server, initialized on the run's first call to that server, and SHALL declare no client capabilities. When the upstream answers 404 for an expired session, the gateway SHALL initialize a new session and retry the call once. It SHALL end a session (`DELETE`) when the run leaves the snapshot, when the server's configuration changes, or after 10 min without a call.

#### Scenario: First call
- **WHEN** run R makes its first call to a `context7__` tool
- **THEN** the gateway sends `initialize` to the `context7` upstream with empty client capabilities, then the call

#### Scenario: Expired upstream session
- **WHEN** the upstream answers a call with 404 for R's session
- **THEN** the gateway initializes a new session and sends the call once more; a second 404 gives outcome `upstream-error`

#### Scenario: Run ends
- **WHEN** run R leaves the snapshot
- **THEN** the gateway sends `DELETE` for R's upstream sessions

#### Scenario: Idle session
- **WHEN** run R made no call to `context7` for 10 min
- **THEN** the gateway sends `DELETE` for that session, and R's next call initializes a new one

### Requirement: Server-initiated requests refused
The gateway SHALL answer every request an upstream sends to its client (sampling, elicitation, roots) with JSON-RPC error `-32601`, and SHALL drop upstream notifications (progress, logging). Nothing an upstream sends SHALL reach the harness except the result of its call.

#### Scenario: Upstream asks for sampling
- **WHEN** the upstream sends `sampling/createMessage` while answering a call
- **THEN** the gateway answers it with error `-32601` and the harness sees only the call result

### Requirement: Size limits
The endpoint SHALL answer a request body over 1 MiB with 413 and contact no upstream. An upstream response over 4 MiB SHALL end the upstream request and give a tool result with `isError: true` (`result too large`), outcome `too-large`.

#### Scenario: Large request
- **WHEN** the harness sends a `tools/call` with a 2 MiB body
- **THEN** the gateway answers 413 and no upstream is contacted

#### Scenario: Large result
- **WHEN** an upstream answers a call with a 6 MiB result
- **THEN** the harness receives `isError: true` with `result too large` and the record has outcome `too-large`

### Requirement: Timeouts and concurrency
The gateway SHALL use a connect timeout of 10 s and a call timeout of 120 s; a timed-out call SHALL get `isError: true` (`upstream timeout`), outcome `timeout`. A run SHALL have at most 8 calls in progress per gateway pod; a further call SHALL get `isError: true` (`too many concurrent calls`), outcome `rejected`, without contacting an upstream.

#### Scenario: Slow upstream
- **WHEN** an upstream sends no result for 120 s
- **THEN** the harness receives `isError: true` with `upstream timeout`

#### Scenario: Ninth concurrent call
- **WHEN** a run has 8 calls in progress on one gateway pod and starts a 9th there
- **THEN** the 9th fails at once without reaching an upstream

### Requirement: Cancelled calls
When the harness closes the connection of a call in progress, the gateway SHALL cancel the upstream request and record outcome `cancelled`. A `notifications/cancelled` that reaches the gateway pod holding the call SHALL do the same and forward the cancellation to the upstream; on another pod it SHALL be ignored.

#### Scenario: Harness disconnects
- **WHEN** the harness closes the connection while a call waits for the upstream
- **THEN** the upstream request is cancelled, the slot is free and the record has outcome `cancelled`

### Requirement: Upstream destination rules
Upstream connections SHALL follow the egress proxy's destination rules: the gateway resolves the host itself and refuses the connection if any resolved address is private, link-local, loopback or in `gateway.blockedCidrs`, and connects to `http://` URLs only while `network.allowPlainHttp` is on. A refused destination SHALL give `isError: true`, outcome `upstream-error`. No `http-host` allowlist entry SHALL be needed for a registered server.

#### Scenario: Name resolves to a private address
- **WHEN** the host of server `docs` resolves to `203.0.113.9` and `10.0.0.5`
- **THEN** the call fails without a connection and the record has outcome `upstream-error`

### Requirement: Call records
The gateway SHALL report one record per `tools/call` to the control plane: run, server, tool, outcome (`ok`, `tool-error`, `rejected`, `upstream-error`, `timeout`, `too-large`, `cancelled`), start time, duration, request and response size in bytes. Records SHALL NOT contain arguments, results or headers. Reporting SHALL be batched and retried; when the buffer is full the oldest records SHALL be dropped and counted in `df.gateway.mcp.calls_dropped`.

#### Scenario: Tool error
- **WHEN** an upstream returns a result with `isError: true`
- **THEN** the record has outcome `tool-error`

#### Scenario: Control plane briefly unreachable
- **WHEN** the control plane is unreachable for 10 s while runs make calls
- **THEN** the records of those calls are delivered after it is reachable again

### Requirement: No payloads in logs
The gateway SHALL NOT log credentials, call arguments or results. Logs SHALL name run, server, tool and outcome only.

#### Scenario: Debug logging
- **WHEN** the gateway runs with `RUST_LOG=debug` and a call carries an API token in its arguments
- **THEN** no log line contains the token
