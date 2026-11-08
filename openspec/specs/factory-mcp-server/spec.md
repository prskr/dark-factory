# factory-mcp-server Specification

## Purpose

Exposes the factory tools to the harness as a standard stdio MCP server named `factory`, without any network listener in the sandbox, forwarding calls to the control plane (ADR-0027).

## Requirements

### Requirement: Stdio entry point bridged over a Unix socket
`factory-agent mcp` SHALL speak MCP over stdio to the harness and relay to the parent runtime through a Unix socket under `/run/factory`. The runtime SHALL open no TCP listener.

#### Scenario: Harness starts the server
- **WHEN** the harness launches `/opt/factory/factory-agent mcp`
- **THEN** MCP `initialize` succeeds and the runtime has no listening TCP socket

### Requirement: Tool list from Assign
`tools/list` SHALL return exactly `Assign.tools` with names, titles, descriptions, input/output schemas and annotations, answered locally.

#### Scenario: Four tools
- **WHEN** the harness lists tools
- **THEN** it sees `update_pull_request`, `comment_on_issue`, `report_progress` and `complete_task`

### Requirement: Calls forwarded with idempotency key
Each `tools/call` SHALL become a `ToolCallRequest` with a new UUIDv7 `call_id`; the `ToolCallResult` SHALL be returned to the harness as the MCP result (content, structured content, `isError`). Text and image content blocks SHALL both be passed on, images as MCP image content with their MIME type.

#### Scenario: Error result
- **WHEN** the control plane answers with `is_error: true` and text "this task has no linked issue"
- **THEN** the harness receives an MCP result with `isError: true` and that text

#### Scenario: Call during a reconnect
- **WHEN** the stream drops while a call is pending
- **THEN** the call is re-sent with the same `call_id` after reconnecting and the harness receives one result

#### Scenario: Image result
- **WHEN** the control plane answers `read_artifact` with a text block and a PNG image block
- **THEN** the harness receives an MCP result with a text content item and an image content item of type `image/png`

### Requirement: Artifact files read by the runtime
For `write_artifact` with `path`, the runtime SHALL read the file itself and send its bytes as the request's attachment, without `path`. The path SHALL be absolute, SHALL resolve beneath the workspace or `/tmp` without passing through any symlink, and SHALL name a regular file of at most 8 MiB, checked before reading. Otherwise the call SHALL fail locally with an error result and nothing SHALL be sent.

#### Scenario: Screenshot in /tmp
- **WHEN** the harness calls `write_artifact` with `path: "/tmp/login.png"`, a regular file
- **THEN** the control plane receives the request with the file's bytes as attachment and no `path`

#### Scenario: Symlinked directory
- **WHEN** `path` is `/data/repo/certs/client.key` and `/data/repo/certs` is a symlink to the runtime's certificate directory
- **THEN** the call fails with an error that the path passes through a symlink, and nothing is read

#### Scenario: Outside the allowed roots
- **WHEN** `path` is `/etc/passwd`
- **THEN** the call fails with an error naming the allowed roots

#### Scenario: Directory
- **WHEN** `path` names a directory in the workspace
- **THEN** the call fails with an error that the path is not a regular file

### Requirement: Early type check in the runtime
Before sending a `write_artifact` call, the runtime SHALL check the content against the artifact types and return an error result for unsupported content without sending it. The control plane's own check SHALL stay authoritative.

#### Scenario: PDF from a file
- **WHEN** the harness calls `write_artifact` with `path` naming a PDF
- **THEN** the call fails locally with an unsupported-type error naming the allowed types
