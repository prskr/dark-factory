# ADR-0032: Third-party MCP servers through the gateway, with tool catalog filtering

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0011](0011-authentication-authorization-personas.md), [ADR-0027](0027-agent-protocol-delivery-and-typed-tool-calls.md), [ADR-0029](0029-egress-gateway-and-generated-network-policies.md)

## Context

Besides the factory tools, agents use third-party MCP servers, e.g. forgejo-mcp with more than 100 tools. Calling them is pass-through traffic with injected credentials, like inference (ADR-0029): payloads can be large, calls long-running, and MCP over HTTP is a protocol harnesses speak natively. Persona tool grants (ADR-0011) alone would show every upstream tool everywhere, and tools added by an upstream update would appear unnoticed.

## Decision

- **Gateway MCP endpoint:** the gateway hosts an MCP endpoint (Streamable HTTP) that aggregates the upstream servers a run may use. The runtime registers it in the harness configuration as a second MCP server, next to its own factory server. The sandbox is identified by pod IP; the gateway injects upstream credentials, enforces the run's tool set on `tools/list` and `tools/call`, offers the `search_tools` / `load_server` meta-tools when the set is large, and reports one record per call (run, server, tool, status, duration, size) to the control plane.
- **Factory tools are unaffected:** they stay in the control plane on the agent stream (ADR-0027).
- **Tool catalog per upstream server:** the control plane discovers each server's tools (`tools/list`) on registration, on demand and periodically, and stores a snapshot (name, description, annotations, schema hash).
- **Instance-level filter per server** decides which tools are considered at all: explicit selection or glob patterns (e.g. `issue_*`, `!*delete*`), optionally only tools annotated read-only. Tools that appear after an upstream update are disabled by default and flagged in the UI; changed schemas are flagged too.
- **Grants choose from the enabled catalog only:** a persona is granted a whole server ("all enabled tools of X") or individual tools; a project can restrict further. A run's tool set is enabled catalog ∩ persona grants ∩ project restrictions, pushed to the gateway with `PolicyWatch`.

## Consequences

- The UI only shows curated tools; model context gets smaller tool lists.
- Upstream updates can't silently grant new capabilities.
- Upstream servers keep MCP features beyond tools (resources, prompts), since the protocol is native end to end.
- The gateway gains an MCP server and client (`rmcp`) and per-run session handling; harnesses see two MCP servers.
- Not part of iteration 1.

## Alternatives considered

- **Proxying upstream MCP through the runtime and the agent stream**: puts the control plane into a data path and needs per-run MCP session state there.
- **Harness connecting to upstream servers directly**: credentials in the sandbox, no per-run filtering.
- **Filtering only through persona grants**: every grant dialog lists the full catalog; new upstream tools slip into "all tools" grants.
