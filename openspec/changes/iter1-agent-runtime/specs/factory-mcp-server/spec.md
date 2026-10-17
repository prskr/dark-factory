# Spec Delta

## Purpose

Exposes the factory tools to the harness as a standard stdio MCP server named `factory`, without any network listener in the sandbox, forwarding calls to the control plane (ADR-0027).

## ADDED Requirements

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
Each `tools/call` SHALL become a `ToolCallRequest` with a new UUIDv7 `call_id`; the `ToolCallResult` SHALL be returned to the harness as the MCP result (content, structured content, `isError`).

#### Scenario: Error result
- **WHEN** the control plane answers with `is_error: true` and text "this task has no linked issue"
- **THEN** the harness receives an MCP result with `isError: true` and that text

#### Scenario: Call during a reconnect
- **WHEN** the stream drops while a call is pending
- **THEN** the call is re-sent with the same `call_id` after reconnecting and the harness receives one result
