## Purpose

The gateway's MCP endpoint for third-party tools: it identifies the calling run, offers the tools of all upstream MCP servers enabled for the run as one MCP server, forwards calls with the upstream credential and reports one record per call, so that no upstream credential is needed in a sandbox.

## ADDED Requirements

### Requirement: MCP endpoint
The gateway SHALL serve one MCP server (Streamable HTTP transport) at `https://<gateway host>:<git port>/mcp` with the certificate of the instance CA, on the same listener as the Git proxy and the inference endpoint. It SHALL answer `POST` with `application/json`, answer `GET` with 405 because it sends no server-initiated messages, and answer `DELETE` with 204.

#### Scenario: Initialize
- **WHEN** the runtime relay of run R sends an MCP `initialize` request to `POST https://<gateway>:8443/mcp`
- **THEN** the gateway answers with server capabilities `tools` only (`listChanged: false`) and an `Mcp-Session-Id` bound to R, and ignores the client capabilities in the request

#### Scenario: Server stream requested
- **WHEN** a client sends `GET /mcp`
- **THEN** the gateway answers 405

#### Scenario: Client ends its session
- **WHEN** the client sends `DELETE /mcp` with its session id
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
- **WHEN** a client sends `tools/list` without `Mcp-Session-Id`
- **THEN** the gateway answers 400

#### Scenario: Request reaches another gateway pod
- **WHEN** run R initialized its session on gateway pod A and its next request reaches pod B
- **THEN** pod B serves the request

### Requirement: Protocol version
The endpoint SHALL answer `initialize` with the protocol version the client requested if it supports it, otherwise with the latest version it supports. A request with an unsupported `MCP-Protocol-Version` header SHALL get 400. JSON-RPC batches SHALL get 400.

#### Scenario: Unsupported version header
- **WHEN** a client sends `tools/list` with `MCP-Protocol-Version: 1999-01-01`
- **THEN** the gateway answers 400

#### Scenario: Batch
- **WHEN** a client sends a JSON array of two requests
- **THEN** the gateway answers 400 and contacts no upstream

### Requirement: Only tools
The endpoint SHALL answer `initialize`, `ping`, `tools/list` and `tools/call`, accept the `notifications/initialized` and `notifications/cancelled` notifications, and answer every other method (resources, prompts, completion, logging) with JSON-RPC error `-32601` (method not found).

#### Scenario: Resources requested
- **WHEN** a client sends `resources/list`
- **THEN** the gateway answers with error `-32601` and contacts no upstream

### Requirement: Aggregated tool list
`tools/list` SHALL ask every server enabled for the run for its tools (all pages, in parallel) and return them in one page, each named `<server>__<tool>` with only its `title`, `description`, `inputSchema`, `outputSchema` and `annotations`. Of tools with the same name from one server, only the first SHALL be listed. A server that fails twice in a row or exceeds the list limits SHALL be left out; the others SHALL still be listed.

#### Scenario: Duplicate name upstream
- **WHEN** `wiki` lists two tools named `search`
- **THEN** the list contains one `wiki__search`, the first one

#### Scenario: Transient failure
- **WHEN** `wiki` answers the first `tools/list` with 503 and the retry succeeds
- **THEN** the list contains the `wiki__` tools

#### Scenario: Two servers
- **WHEN** project `DF` has `context7` (tool `get-library-docs`) and `wiki` (tool `search`) enabled and a `DF` run lists tools
- **THEN** it receives `context7__get-library-docs` and `wiki__search` with the upstreams' current definitions

#### Scenario: Extra fields not served
- **WHEN** an upstream tool has a `_meta` and an `icons` field
- **THEN** the listed tool has neither

#### Scenario: One upstream down
- **WHEN** `wiki` refuses the connection twice while the run lists tools
- **THEN** the list contains the `context7__` tools only, and a record for `wiki` with tool `tools/list` and outcome `upstream-error` is reported

### Requirement: Tool list deadline and cache
One `tools/list` SHALL answer within 20 s: an upstream list SHALL time out after 15 s (outcome `timeout`), and servers not done by the deadline SHALL be left out. The gateway pod SHALL reuse a run's aggregated list for 60 s, so repeated `tools/list` requests do not reach the upstreams again. A change of the run's enabled servers, or of the configuration of one of them, SHALL drop the run's cached list.

#### Scenario: Slow upstream during tools/list
- **WHEN** `wiki` sends no list for 15 s while `context7` answers at once
- **THEN** the harness receives the `context7__` tools within 20 s, and a record for `wiki` with tool `tools/list` and outcome `timeout` is reported

#### Scenario: Repeated listing
- **WHEN** the harness lists tools twice within 60 s
- **THEN** the upstreams receive one `tools/list` each

#### Scenario: Server disabled within the cache window
- **WHEN** the harness lists tools, `context7` is disabled for the project 10 s later, and the harness lists again
- **THEN** the second list contains no `context7__` tools

#### Scenario: Public server resolves to a private address during listing
- **WHEN** the host of server `docs` (without "Private network") resolves to `10.0.0.5` while the run lists tools
- **THEN** no connection is opened, the `docs__` tools are left out, and a record with outcome `upstream-error` is reported

### Requirement: Tool names agents can use
The gateway SHALL leave out every upstream tool whose name does not match `^[A-Za-z0-9_-]{1,64}$` or whose name as the harness sees it, `mcp__ext__<server>__<tool>`, exceeds 64 characters, and SHALL log it with server and tool name.

#### Scenario: Name too long
- **WHEN** server `forgejo` lists a tool whose name has 50 characters
- **THEN** the tool is not listed and the gateway logs it

### Requirement: Upstream list limits
The gateway SHALL accept at most 1,000 tools and 4 MiB per server for one `tools/list`, and SHALL leave out a server that exceeds either limit.

#### Scenario: Huge tool list
- **WHEN** an enabled server lists 1,500 tools
- **THEN** none of its tools is listed, and a record with tool `tools/list` and outcome `too-large` is reported

### Requirement: Enforcement of the enabled servers
For `tools/call` the endpoint SHALL split the name at the first `__` and check that the prefix names a server enabled for the run. Otherwise the call SHALL get a tool result with `isError: true` and the text `tool <name> is not available in this run`, SHALL be recorded with outcome `rejected`, and SHALL NOT contact any upstream.

#### Scenario: Server of another project
- **WHEN** a run of `DF` calls `billing__export`, and `billing` is registered but not enabled for `DF`
- **THEN** the call fails with `isError: true`, no request reaches the `billing` upstream, and a record with outcome `rejected` is reported

#### Scenario: Server disabled after listing
- **WHEN** the harness listed `context7__get-library-docs`, then `context7` is disabled for the project, then the harness calls it
- **THEN** the call is rejected

### Requirement: Calls forwarded unchanged
A permitted call SHALL be sent to its server's upstream as `tools/call` with the name after the prefix and the arguments unchanged. The upstream's result (content, structured content, `isError`) SHALL be returned unchanged apart from the credential redaction; an upstream JSON-RPC error SHALL be returned as a tool result with `isError: true` and the error message, outcome `tool-error`.

#### Scenario: Successful call
- **WHEN** the harness calls `context7__get-library-docs` with `{"libraryId": "/tokio-rs/tokio"}`
- **THEN** the `context7` upstream receives `tools/call` for `get-library-docs` with the same arguments, and the harness receives its result

#### Scenario: Unknown tool upstream
- **WHEN** the harness calls `context7__no-such-tool` and the upstream answers with a JSON-RPC error
- **THEN** the harness receives `isError: true` with the upstream's message and the record has outcome `tool-error`

### Requirement: Upstream failures
When the upstream cannot be connected to, the TLS handshake fails, or it answers with an HTTP error status, a non-MCP body or a redirect, the call SHALL get `isError: true` with a fixed message naming the cause (never the credential), outcome `upstream-error`. Redirects SHALL NOT be followed.

#### Scenario: Upstream answers 401
- **WHEN** the upstream answers a call with 401
- **THEN** the harness receives `isError: true` with "upstream refused the credential (401)" and the record has outcome `upstream-error`

#### Scenario: Redirect
- **WHEN** the upstream answers a call with `307` and `Location: http://10.0.0.5/`
- **THEN** no request is sent to `10.0.0.5` and the call fails with outcome `upstream-error`

#### Scenario: Upstream down
- **WHEN** the upstream host refuses the connection
- **THEN** the call fails with outcome `upstream-error`

### Requirement: Credential injection
The gateway SHALL build every upstream request itself and SHALL forward no header of the sandbox's request. It SHALL inject the server's credential as defined by its scheme (header or query parameter), and SHALL never return the credential, the upstream URL or upstream response headers to the client, or write the credential or the upstream URL's query to logs.

#### Scenario: Sandbox sends its own credential
- **WHEN** a request to `/mcp` carries `Authorization: Bearer stolen` and `CONTEXT7_API_KEY: x`
- **THEN** the upstream request carries only the server's configured credential header and neither value from the sandbox

#### Scenario: Server without a credential scheme
- **WHEN** a snapshot carries a server whose credential scheme is not set
- **THEN** the gateway treats it like a server without a credential: its tools are left out, calls fail with outcome `upstream-error`, and no upstream is contacted

### Requirement: Credential removed from returned content
Before returning anything from an upstream to the sandbox (tool lists, results, error messages), the gateway SHALL replace every occurrence of the server's credential value, raw and URL-encoded, with `[redacted]`.

#### Scenario: Upstream echoes the token
- **WHEN** server `penpot` (scheme `query`) answers a call with a JSON-RPC error whose message contains the request URL with `token=<value>`
- **THEN** the harness receives the message with `token=[redacted]`, and the transcript never holds the value

#### Scenario: Token in a tool result
- **WHEN** an upstream returns a tool result whose text contains the server's bearer token
- **THEN** the harness receives the text with `[redacted]` in its place

#### Scenario: Query credential not leaked
- **WHEN** server `penpot` has scheme `query` and its upstream answers a call with 500
- **THEN** the harness receives the fixed message "upstream error (500)" without the URL, and no log line contains the token

#### Scenario: Credential missing from the snapshot
- **WHEN** the server's credential is not in the snapshot (unresolvable Secret)
- **THEN** the call fails with `isError: true` and a fixed message, outcome `upstream-error`, its tools are left out of `tools/list`, and no upstream is contacted

### Requirement: Upstream sessions
The gateway SHALL be the MCP client of each upstream with one session per gateway pod, run and server, initialized on first use, and SHALL declare no client capabilities. When the upstream answers 404 for an expired session, the gateway SHALL initialize a new session and retry once. It SHALL end a session (`DELETE`) when the run leaves the snapshot, when the server's configuration changes, or after 10 min without use.

#### Scenario: First call
- **WHEN** run R uses `context7` for the first time on a gateway pod
- **THEN** the gateway sends `initialize` to the `context7` upstream with empty client capabilities, waits for its successful response, sends `notifications/initialized`, then sends the request

#### Scenario: Expired upstream session
- **WHEN** the upstream answers a call with 404 for R's session
- **THEN** the gateway initializes a new session, completes the handshake with `notifications/initialized`, and sends the call once more; a second 404 gives outcome `upstream-error`

#### Scenario: Run ends
- **WHEN** run R leaves the snapshot
- **THEN** the gateway sends `DELETE` for R's upstream sessions

#### Scenario: Idle session
- **WHEN** run R did not use `context7` for 10 min
- **THEN** the gateway sends `DELETE` for that session, and R's next call initializes a new one

### Requirement: Server-initiated requests refused
The gateway SHALL answer every request an upstream sends to its client (sampling, elicitation, roots) with JSON-RPC error `-32601`, and SHALL drop upstream notifications (progress, logging). Nothing an upstream sends SHALL reach the sandbox except tool lists and call results.

#### Scenario: Upstream asks for sampling
- **WHEN** the upstream sends `sampling/createMessage` while answering a call
- **THEN** the gateway answers it with error `-32601` and the harness sees only the call result

### Requirement: Size limits
The endpoint SHALL answer a request body over 1 MiB with 413 and contact no upstream. An upstream call response over 4 MiB SHALL end the upstream request and give a tool result with `isError: true` (`result too large`), outcome `too-large`.

#### Scenario: Large request
- **WHEN** a client sends a `tools/call` with a 2 MiB body
- **THEN** the gateway answers 413 and no upstream is contacted

#### Scenario: Large result
- **WHEN** an upstream answers a call with a 6 MiB result
- **THEN** the harness receives `isError: true` with `result too large` and the record has outcome `too-large`

### Requirement: Timeouts and concurrency
The gateway SHALL use a connect timeout of 10 s and a timeout of 120 s per call; a timed-out call SHALL get `isError: true` (`upstream timeout`), outcome `timeout`. A run SHALL have at most 8 calls in progress per gateway pod; a further call SHALL get `isError: true` (`too many concurrent calls`), outcome `rejected`, without contacting an upstream.

#### Scenario: Slow upstream
- **WHEN** an upstream sends no result for 120 s
- **THEN** the harness receives `isError: true` with `upstream timeout`

#### Scenario: Ninth concurrent call
- **WHEN** a run has 8 calls in progress on one gateway pod and starts a 9th there
- **THEN** the 9th fails at once without reaching an upstream

### Requirement: Cancelled calls
When the client closes the connection of a call in progress, the gateway SHALL cancel the upstream request and record outcome `cancelled`. A `notifications/cancelled` that reaches the gateway pod holding the call SHALL do the same and forward the cancellation to the upstream; on another pod it SHALL be ignored.

#### Scenario: Client disconnects
- **WHEN** the client closes the connection while a call waits for the upstream
- **THEN** the upstream request is cancelled, the slot is free and the record has outcome `cancelled`

#### Scenario: Cancel notification on the holding pod
- **WHEN** a client sends `notifications/cancelled` for a call in progress and it reaches the pod holding the call
- **THEN** the pod forwards the cancellation to the upstream, closes the pending call's response without a JSON-RPC result, frees the slot and records `cancelled`

### Requirement: Destination check for public servers
For a server without "Private network", the gateway SHALL resolve the host itself and refuse the connection if any resolved address is private, link-local, loopback or in `gateway.blockedCidrs`. For a server with "Private network" it SHALL skip this check. `http://` URLs SHALL be used only while `network.allowPlainHttp` is on. No `http-host` allowlist entry SHALL be needed for a registered server.

#### Scenario: Public server resolves to a private address
- **WHEN** the host of server `docs` (without "Private network") resolves to `203.0.113.9` and `10.0.0.5`
- **THEN** the call fails without a connection and the record has outcome `upstream-error`

#### Scenario: Internal server
- **WHEN** the host of server `wiki` (with "Private network") resolves to `10.20.0.7`
- **THEN** the gateway connects and forwards the call

### Requirement: Call records
The gateway SHALL report one record per `tools/call`, and one per server left out of a `tools/list` (tool `tools/list`), to the control plane: run, server, tool, outcome (`ok`, `tool-error`, `rejected`, `upstream-error`, `timeout`, `too-large`, `cancelled`), start time, duration, request and response size. Records SHALL NOT contain arguments, results or headers.

#### Scenario: Tool error
- **WHEN** an upstream returns a result with `isError: true`
- **THEN** the record has outcome `tool-error`

### Requirement: Call record delivery
Reporting SHALL be batched and retried. When the buffer is full the oldest records SHALL be dropped and counted in `df.gateway.mcp.calls_dropped`.

#### Scenario: Control plane briefly unreachable
- **WHEN** the control plane is unreachable for 10 s while runs make calls
- **THEN** the records of those calls are delivered after it is reachable again

### Requirement: No payloads in logs
The gateway SHALL NOT log credentials, call arguments, results or tool descriptions. Logs SHALL name run, server, tool and outcome only.

#### Scenario: Debug logging
- **WHEN** the gateway runs with `RUST_LOG=debug` and a call carries an API token in its arguments
- **THEN** no log line contains the token
