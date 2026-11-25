# Proposal: iter4-mcp-gateway

## Why

Agents can use only the factory tools. Third-party MCP servers (Context7, forgejo-mcp, internal tools) need credentials, and today the only way to use them would be to put the credential into the sandbox, which ADR-0010 forbids. ADR-0032 and iteration 4 decisions 1, 7 and 8 fix the target: one aggregating MCP endpoint in the gateway, HTTP upstreams with static credentials, credentials injected by the gateway.

Iteration 4 planned `iter4-permissions` → `iter4-mcp-catalog` → `iter4-mcp-gateway`. None of these is usable alone, and the catalog and permissions are the larger part. This change is the **initial, end-to-end slice** and needs no earlier change: a board member registers an HTTP MCP server and enables it for a project, and the next run of that project can call **all** of the server's tools through the gateway. Review, filters and grants come later on top (Peter Kurfer, 2026-11-25).

## What Changes

- **MCP servers (control plane).** Board members register an upstream MCP server per instance: name (the tool prefix), Streamable HTTP URL, credential reference (Secret name and key, as for model providers), credential scheme (`bearer` or `header` with name and prefix) and a **"Private network"** switch for servers on private addresses. Changes are audited. The credential never leaves the control plane and the gateway.
- **Enablement per project, opt-in.** The project settings page gets an "MCP servers" tab. A run gets the tools of every server enabled for its project. This amends ADR-0032 (projects there only restrict); per-persona grants come later and intersect with it.
- **Gateway MCP endpoint.** The gateway serves one MCP endpoint (Streamable HTTP) at `/mcp` on the Git proxy's TLS listener, next to `/inference`. It identifies the run by source pod IP. `tools/list` asks every enabled upstream for its tools and returns them together, named `<server>__<tool>`. `tools/call` goes to the upstream of the prefix, with the injected credential; a call to a server not enabled for the run is rejected without contacting any upstream. No header from the sandbox reaches the upstream.
- **All tools are forwarded.** There is no tool selection in this slice: agents see the upstream's current tool list and descriptions. The token's scope is the effective permission; the docs say to register servers with read-only tokens where possible and no forge token with write access until filters and "ask" exist.
- **Upstream sessions and limits.** The gateway is the MCP client of each upstream, one session per gateway pod, run and server. It advertises no client capabilities (no sampling, elicitation or roots) and follows no redirects. Fixed limits: request 1 MiB, result 4 MiB, tool list 1,000 tools and 4 MiB per server, connect 10 s, 15 s per upstream list and 20 s per `tools/list` (lists cached 60 s), call 120 s, 8 concurrent calls per run and gateway pod.
- **Guard for factory credentials.** An MCP server cannot use the credential reference of a project's forge or of a model provider, so the factory's own forge token cannot be handed to an MCP server.
- **Destination check for public servers.** Without "Private network", upstream connections apply the proxy's destination rules (§7 of `design/egress.md`): a name that resolves to a private, link-local or loopback address is refused, so a third party controlling a public server's DNS cannot point the gateway at internal services. With "Private network" the check is skipped for that server. A registered server needs no allowlist entry.
- **Call records.** The gateway reports one record per call, and one per server left out of a tool list (run, server, tool, outcome, duration, request and response size), to the control plane, which stores it with the run and shows it in the run view. Arguments and results are not stored; the transcript already has them.
- **Runtime relay.** `Assign` carries the gateway MCP endpoint when the run has at least one enabled server. The runtime registers a second stdio MCP server `ext` in ACP `session/new` and `session/load`, next to `factory`: `factory-agent mcp-relay` forwards every MCP message unchanged to the gateway endpoint. Stdio MCP servers are mandatory for every ACP agent, so this works for every harness without HTTP MCP support in the harness.
- **Contracts.** `darkfactory.gateway.v1`: MCP servers and credentials in the policy snapshot, the enabled servers per sandbox entry, an MCP call report RPC. `darkfactory.agent.v1`: the MCP endpoint in `Assign`. New fields in existing messages get placeholder numbers.
- **Docs.** A new security page for the MCP endpoint, a how-to for registering a server, the project settings page, the configuration and architecture references.

Out of scope:
- Reviewing and pinning tool definitions, new and changed tools flagged, periodic discovery, instance filters (selection, globs, read-only only): `iter4-mcp-catalog`, roadmap iteration 4 ("Third-party MCP servers through the gateway").
- Per-persona grants and per-project restrictions to single tools: `iter4-mcp-catalog`, roadmap iteration 4, built on the per-persona tool permissions (allowed, ask, denied) of "Full permission model (ADR-0011)".
- The `tool-access` approval: `iter4-mcp-catalog`, roadmap iteration 4.
- Stdio upstream servers: the stdio → HTTP bridge (registered as a "Private network" server once the bridge exists) and project-scoped `.mcp.json` servers with the `repository-mcp-server` approval: `iter4-mcp-catalog`, roadmap iteration 4 (decision 8).
- `search_tools` / `load_server` for large tool sets: backlog row added by this change ("MCP meta-tools …").
- One credential per project or per persona for the same server: backlog row added by this change ("MCP server credentials per project …").
- Rate limits per run against the shared upstream quota: backlog row added by this change ("MCP call rate limits per run").
- MCP resources and prompts: backlog row added by this change ("MCP resources and prompts through the gateway"). Server-initiated requests (sampling, elicitation, roots): rejected; the gateway refuses them.
- OAuth-based upstream servers: roadmap backlog (existing row, decision 7).
- Live `tools/list_changed` notifications to running sessions: rejected for now; enablement changes apply to calls at once and to the tool list from the next run.
- Storing call arguments or results: rejected; the transcript holds them.
- An HTTP MCP server registered directly in the harness: rejected; the stdio relay works for every ACP harness (design D8).

## Capabilities

### New Capabilities
- `mcp-servers`: registration of upstream MCP servers, their credential and the "Private network" switch, enablement per project, call records in the run view, audit.
- `mcp-endpoint`: the gateway's MCP endpoint: caller identification, aggregated tool list, enforcement of the enabled servers, credential injection, upstream sessions, limits, destination check, call records.

### Modified Capabilities
- `egress-policy-distribution`: the policy snapshot carries the MCP servers of active runs and their credentials, and each sandbox entry the servers enabled for the run.
- `run-orchestration`: `Assign` carries the gateway MCP endpoint.
- `acp-harness`: the runtime registers the stdio relay `ext` in `session/new` and `session/load`.

## Impact

- `crates/df-gateway`: new `mcp` module (endpoint for the harness, client for upstreams on the existing `UpstreamConnector`, with the proxy's `BlockedRanges` check for public servers), call report buffer. `rmcp` (already a workspace dependency) for the MCP message types; the transport is our own on the existing hyper server (design D4).
- `crates/df-gateway-control`: snapshot building for MCP servers, call report handling.
- `crates/df-db`: migration for `mcp_servers`, `project_mcp_servers`, `mcp_calls`; queries.
- `crates/df-web`: MCP servers settings page, project settings tab, run view call list.
- `crates/df-agent-channel` (`Assign`), `crates/factory-agent` (`mcp-relay` subcommand, `session/new`, `session/load`).
- `contract/proto/darkfactory/gateway/v1/gateway.proto`, `contract/proto/darkfactory/agent/v1/agent.proto`.
- `product/iterations/iteration-4.md` and `roadmap.md`: the MCP rows put this slice first and `iter4-mcp-catalog` after it; four backlog rows.
- Helm: no new values. The gateway's network policy restricts only ingress, so private MCP servers are reachable.
