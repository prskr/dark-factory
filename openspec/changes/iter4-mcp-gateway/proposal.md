# Proposal: iter4-mcp-gateway

## Why

Agents can use only the factory tools. Third-party MCP servers (Context7, forgejo-mcp, internal tools) need credentials, and today the only way to use them would be to put the credential into the sandbox, which ADR-0010 forbids. ADR-0032 and iteration 4 decisions 1, 7 and 8 fix the target: one aggregating MCP endpoint in the gateway, HTTP upstreams with static credentials, credentials injected by the gateway.

Iteration 4 plans this as two changes, `iter4-mcp-catalog` (control plane) and then `iter4-mcp-gateway`. Neither is useful alone, and the catalog is the larger half. This change is the **initial, end-to-end slice**: a board member registers one HTTP MCP server, picks the tools agents may see, enables the server for a project, and the next run of that project can call those tools through the gateway. The full catalog (discovery, filters, grants, approvals) follows as named changes and builds on this slice instead of being a prerequisite for it.

## What Changes

- **MCP servers (control plane).** Board members register an upstream MCP server per instance: name (the tool prefix), Streamable HTTP URL, credential reference (Secret name and key, as for model providers) and credential scheme (`bearer` or `header` with name and prefix). Changes are audited. The credential never leaves the control plane and the gateway.
- **Pinned tool list.** On "Fetch tools" the control plane calls the upstream's `tools/list` and shows every tool with its description, input schema and annotations. The board member selects the tools agents may use; nothing is selected by default. The selection is stored with exactly the definitions of the fetch the board member saw (**pinned**), at most 100 tools and 256 KiB per server and 2 MiB per instance. The gateway serves only the pinned definitions; upstream changes have no effect until a board member fetches again. A refetch marks new, changed and removed tools; new and changed tools come back unselected. Tools not annotated read-only are marked "may write", with the warning that their writes bypass push checks and approvals.
- **Enablement per project, opt-in.** The project settings page gets an "MCP servers" tab. A run gets the pinned tools of every server enabled for its project. This amends ADR-0032 (projects there only restrict); per-persona grants come later and intersect with it.
- **Gateway MCP endpoint.** The gateway serves one MCP endpoint (Streamable HTTP) at `/mcp` on the Git proxy's TLS listener, next to `/inference`. It identifies the run by source pod IP, answers `initialize` and `tools/list` from the policy snapshot, with tool names `<server>__<tool>`, and forwards `tools/call` to the upstream with the injected credential. A call to a tool outside the run's set is rejected without contacting the upstream. No header from the sandbox reaches the upstream.
- **Upstream sessions and limits.** The gateway is the MCP client of each upstream, one session per gateway pod, run and server, opened on the first call. It advertises no client capabilities (no sampling, elicitation or roots) and follows no redirects. Fixed limits: request 1 MiB, result 4 MiB, connect 10 s, call 120 s, 8 concurrent calls per run and gateway pod. The gateway's `PolicyWatch` decode limit rises to 16 MiB, so pinned definitions can never make a snapshot undecodable.
- **Egress for upstream calls.** Upstream connections apply the proxy's destination rules (§7 of `design/egress.md`): the name is resolved by the gateway, private, link-local and cluster addresses are refused, `https` only unless `network.allowPlainHttp`. An in-cluster server (for example a stdio → HTTP bridge) needs a per-server opt-in that comes with the bridge. A registered server needs no allowlist entry: registering it is the allow decision, as for model providers.
- **Call records.** The gateway reports one record per call (run, server, tool, outcome, duration, request and response size) to the control plane, which stores it with the run and shows it in the run view. Arguments and results are not stored; the transcript already has them.
- **Runtime.** `Assign` carries the gateway MCP endpoint when the run has at least one enabled server. The runtime registers it in ACP `session/new` and `session/load` as HTTP MCP server `ext`, next to `factory`, if the agent advertises `mcpCapabilities.http`; otherwise it emits a transcript notice and starts without it.
- **Contracts.** `darkfactory.gateway.v1`: MCP servers with pinned tools and credentials in the policy snapshot, the enabled servers per sandbox entry, an MCP call report RPC. `darkfactory.agent.v1`: the MCP endpoint in `Assign`. New fields in existing messages get placeholder numbers.
- **Docs.** A new security page for the MCP endpoint, a how-to for registering a server, the project settings page, the configuration and architecture references.

Out of scope:
- Periodic and automatic tool discovery, catalog snapshots with history, instance filters (globs, "read-only only"): `iter4-mcp-catalog`, roadmap iteration 4 ("Third-party MCP servers through the gateway"). The pinned list of this change is its first version.
- Per-persona grants and per-project restrictions to single tools: `iter4-mcp-catalog`, roadmap iteration 4, built on the per-persona tool permissions (allowed, ask, denied) of "Full permission model (ADR-0011)". Until "ask" exists, the docs say not to select write tools of servers that reach the forge (design, Security).
- The `tool-access` approval: `iter4-mcp-catalog`, roadmap iteration 4.
- `search_tools` / `load_server` for large tool sets: backlog row added by this change ("MCP meta-tools for large tool sets"). The pinned selection keeps sets small until then.
- Stdio servers: the stdio → HTTP bridge and project-scoped `.mcp.json` servers with the `repository-mcp-server` approval: `iter4-mcp-catalog`, roadmap iteration 4 (decision 8).
- OAuth-based upstream servers: roadmap backlog (existing row, decision 7).
- One credential per project or per persona for the same server: backlog row added by this change ("MCP server credentials per project").
- Rate limits per run against the shared upstream quota: backlog row added by this change ("MCP call rate limits per run").
- MCP resources and prompts: backlog row added by this change ("MCP resources and prompts through the gateway"). Server-initiated requests (sampling, elicitation, roots): rejected; the gateway refuses them.
- Live `tools/list_changed` notifications to running sessions: rejected for now; removals apply to calls at once, additions from the next run.
- Storing call arguments or results: rejected; the transcript holds them.

## Capabilities

### New Capabilities
- `mcp-servers`: registration of upstream MCP servers, their credential, the pinned tool list with refetch and change marks, enablement per project, audit.
- `mcp-endpoint`: the gateway's MCP endpoint: caller identification, aggregated tool list, tool set enforcement, credential injection, upstream sessions, limits, egress rules for upstream connections, call records.

### Modified Capabilities
- `egress-policy-distribution`: the policy snapshot carries MCP servers with pinned tools and credentials, and each sandbox entry the servers enabled for the run; the gateway accepts snapshots up to 16 MiB.
- `run-orchestration`: `Assign` carries the gateway MCP endpoint.
- `acp-harness`: the runtime registers the gateway MCP endpoint in `session/new` and `session/load`.
- `opencode-harness`: OpenCode receives the gateway MCP endpoint the same way (checked by the spike in task 1.2).

## Impact

- `crates/df-gateway`: new `mcp` module (endpoint for the harness, client for upstreams on the existing `UpstreamConnector` with the proxy's `BlockedRanges` check), call report buffer. `rmcp` (already a workspace dependency) for the MCP model types; the transport is our own on the existing hyper server (design D4, spike 1.3).
- `crates/df-gateway-control`: snapshot building for MCP servers, call report handling, the "Fetch tools" client (reuses the gateway's MCP client in-process).
- `crates/df-db`: migration for `mcp_servers`, `mcp_server_tools`, `project_mcp_servers`, `mcp_calls`; queries.
- `crates/df-web`: MCP servers settings page, project settings tab, run view call list.
- `crates/df-agent-channel` (`Assign`), `crates/factory-agent` (`session/new`, `session/load`).
- `contract/proto/darkfactory/gateway/v1/gateway.proto`, `contract/proto/darkfactory/agent/v1/agent.proto`.
- `docs/architecture.md`: `df-gateway-control` uses the gateway's MCP client.
- `product/iterations/iteration-4.md` and `roadmap.md`: the MCP rows change to this slice first, `iter4-mcp-catalog` after it; two backlog rows.
- Helm: no new values.
