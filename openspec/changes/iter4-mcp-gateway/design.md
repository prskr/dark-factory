# Design: iter4-mcp-gateway

## Context

ADR-0032 and `design/egress.md` §10 describe the target: one aggregating MCP endpoint in the gateway, tool catalogs in the control plane, grants per persona. Iteration 4 (decisions 1, 7, 8) fixes HTTP upstreams with static credentials. Iteration 4 also ordered `iter4-permissions` → `iter4-mcp-catalog` → `iter4-mcp-gateway`. On 2026-11-25 Peter Kurfer decided to start with the gateway integration, forward all tools at first, and add review, filters and permissions on top later (D1).

What exists today (observed in the code):

- **Gateway.** `/inference` shares the Git port (8443) and its TLS listener (`df-gateway/src/git/server.rs`, hyper, HTTP/1.1). Runs are found by source pod IP (`Policy::sandbox`). Credentials come in the `PolicyWatch` snapshot (`Snapshot.provider_credentials`, keyed by credential reference id) and are injected in `inference/headers.rs`. Usage records go through a bounded, batched reporter (`inference/report.rs`, `GatewayUsage.Report`).
- **Upstream connections.** `UpstreamConnector` (`df-gateway/src/upstream.rs`) is shared by Git, mirrors and inference. It connects directly with `TcpStream::connect` and does **not** apply the proxy's private-destination check (`resolve::BlockedRanges`, used only by `proxy::Proxy`, which refuses a name if any resolved address is blocked). `BlockedRanges` covers private, loopback and link-local ranges plus `gateway.blockedCidrs` (default `[]`). `UpstreamUrl::parse` accepts `https` only (`http` with `network.allowPlainHttp`) and rejects user information, query and fragment.
- **Gateway network policy** (`deploy/helm/dark-factory/templates/netpol.yaml`) restricts ingress only. The gateway can reach private networks.
- **Credentials.** `credential_refs` point at a Kubernetes Secret name and key; `SecretStore` resolves them at use time (`credential-resolution`).
- **Runtime.** `factory-agent/src/harness.rs` `setup_session()` passes one stdio server, `factory` (`/opt/factory/factory-agent mcp --socket <path>`), to `session/new` and `session/load`. `factory-agent mcp` (`mcp.rs`) is an `rmcp` stdio server that relays to the runtime over a Unix socket; factory tool calls then go over gRPC to the control plane. The harness reads the instance CA from `INSTANCE_CA_PATH`. Landlock restricts files, not network. The transcript marks tool calls `mcp__factory__*` as factory calls, all others as harness calls, and truncates raw input and output at 64 KiB.
- **`rmcp` 3.9** is a workspace dependency with features `client`, `server`, `transport-async-rw`. No Streamable HTTP transport is enabled.
- **Authorization.** Every page sits behind `require_board_member` (role `board-member`). The role `admin` exists already (`df-web/src/auth/mod.rs`, `ADMIN_ROLE`): `require_admin` guards the project webhook and lifecycle subscription routes, and only admins see and save the "Integrations" tab of the project settings (`config_pages.rs`, `Editing::AsAdmin`). Roles come from the ID token's `auth.rolesClaim`. There is no tool permission yet. `docs/authorization.md` (added by this change) describes the concept for contributors.

## Goals / Non-Goals

**Goals:**
- One end-to-end path with no earlier change: register a server → enable for a project → the agent calls a tool through the gateway → call record in the run view.
- Credentials never in the sandbox, the transcript or logs.
- Works for every ACP harness, without HTTP MCP support in the harness.
- Every piece is a subset of the ADR-0032 target, so review, filters and grants (`iter4-mcp-catalog`) are added on top without replacing it.

**Non-Goals:** (proposal "Out of scope" has the full list and roadmap targets)
- Tool review, pinning, filters, discovery flags, persona grants, `tool-access`: `iter4-mcp-catalog` (roadmap iteration 4).
- `search_tools` / `load_server`, per-project credentials, per-run rate limits, MCP resources and prompts: backlog rows added by this change.
- Private-destination check for the inference and Git upstreams: rejected in this change (Q6).

## Decisions

### D1: The slice: gateway first, all tools forwarded

Catalog, filters and grants without the gateway are not testable end to end, and their UI questions are easier to answer once real servers are in use. Everything later is a layer on the gateway: filters and grants only change which tools the gateway lists and allows, not how calls are forwarded.

This slice forwards every tool of an enabled server, with the upstream's current descriptions. Accepted until `iter4-mcp-catalog`:
- Upstream changes reach agents unreviewed: a server can change a description or add a tool, and the next run sees it (ADR-0032 wants to prevent this; it is the first thing the catalog adds).
- Every write tool is available, without push checks or approvals. The credential's scope is the real permission (Security).
- When the catalog arrives, tool definitions move from the gateway's live `tools/list` to the snapshot. The aggregation of live lists (D5) then becomes the discovery source; the per-run list path is replaced. That rework is small.

**Amendments of ADR-0032 and iteration 4 decision 2.** Until `iter4-mcp-catalog`, this slice suspends "upstream updates can't silently grant new capabilities" (ADR-0032) and "tools added upstream are disabled by default" (decision 2): all tools are forwarded unreviewed. Demo step 3 of iteration 4 ("enables a subset", "shows up disabled and flagged") holds only after the catalog. Task 7.2 records this in ADR-0032; `iteration-4.md` notes it at decision 2 and demo step 3. ADR-0032 also has every enabled tool available to granted personas, and projects can *restrict further* (default allow). This change makes projects **opt in** per server (default deny). The reason is the shared credential: enabling a server for a project gives that project's agents everything the credential reaches. With persona grants later, a run's tools become servers enabled for the project ∩ catalog filters ∩ persona grants. Task 7.2 records this in ADR-0032.

Alternatives: permissions and catalog first (the original plan): larger first change, nothing usable until three changes are done. Pinned review in the first slice (the first version of this proposal): about a third more work (fetch page, selection, change marks, race handling, size caps in the snapshot) before the first call works; moved to `iter4-mcp-catalog`.

### D2: Data model

| Table | Columns (main) | Notes |
|---|---|---|
| `mcp_servers` | `id`, `name` UNIQUE, `url` (with path and query), `credential_ref_id` → `credential_refs` ON DELETE RESTRICT, `credential_scheme` CHECK `bearer`\|`header`\|`query`, `credential_header`, `credential_prefix`, `credential_query_param`, `private_network` bool, `config_version`, timestamps | Name immutable (D6). A CHECK ties the scheme to its columns (`header` needs `credential_header`, `query` needs `credential_query_param`, the others are NULL). `config_version` increases on every edit; gateways end upstream sessions when it changes. |
| `project_mcp_servers` | `project_id`, `server_id` → `mcp_servers` (RESTRICT); PK both | Enablement. Delete refused while rows exist. |
| `mcp_calls` | `call_id` UUID PK, `run_id` → `runs` ON DELETE CASCADE, `server_name`, `tool`, `outcome`, `started_at`, `duration_ms`, `request_bytes`, `response_bytes` | `server_name` instead of an FK, so records survive a server deletion. PK makes retried reports idempotent, as `model_usage.request_id`. |

Queries live in a new `df-db` module `mcp_servers`.

### D3: The private-address check is for public servers only

The gateway sits in a privileged place: its network policy limits only ingress, so it reaches the control plane's internal port, PostgreSQL, other services and private networks. A tool call is a request body the agent chooses, and the answer comes back to the agent. If a third party controls a public server's DNS, it can point the name at an internal address later (DNS rebinding) and turn the gateway into a client for internal services.

That attack needs a name the admin did not choose to resolve somewhere unexpected. For a server the admin registers on a private address on purpose, it does not apply. So the check is a per-server switch:

- **"Private network" off (default):** the gateway resolves the host with `SystemResolver`, refuses the connection if `BlockedRanges` covers **any** resolved address (as `proxy.rs`), and connects to a checked address with the original host for SNI.
- **"Private network" on:** no address check; the gateway connects to whatever the name resolves to. For internal servers, and later for stdio → HTTP bridges in the cluster. Use it only for names in DNS you control: it also turns off the rebinding protection, and with it on, the server's name may point at `localhost` or the gateway's own ports. The security page says so.
- **Both:** redirects are never followed (a 3xx could lead anywhere), `http` only with `network.allowPlainHttp`, no `http-host` allowlist entry needed (registering is the allow decision, as for model providers).

Credential injection does not depend on the switch.

Alternative: no check at all, as the inference and Git upstreams. Simpler, but leaves public servers open to rebinding for the price of one boolean.

### D4: Endpoint transport: own thin layer, `rmcp` for types

The endpoint is a small JSON-RPC server: `initialize`, `ping`, `tools/list`, `tools/call`, two notifications, everything else `-32601`. Responses are `application/json` (Streamable HTTP lets a server answer a POST with JSON instead of SSE); there is no server-initiated message, so `GET` is 405. Batches are refused (MCP 2025-06-18 removed them).

The upstream side is a small client: `POST` JSON-RPC, accept a JSON or an SSE response, read the SSE stream until the response with the request's id, answer server requests on the stream with `-32601`, drop notifications, keep `Mcp-Session-Id`, send `MCP-Protocol-Version`, never follow redirects.

Both run on what the gateway already has: the hyper server of the Git port and `UpstreamConnector`, so TLS, timeouts, `LimitedBody` and the destination check stay in one place. `rmcp`'s model types give the wire format. Its Streamable HTTP transports bring axum and reqwest, a second HTTP stack each, which would bypass our checks. Spike 1.1 confirms the model types are usable alone.

### D5: Aggregated tool list

On `tools/list` the gateway asks each enabled server for `tools/list` (all pages; 1,000 tools and 4 MiB per server) in parallel, keeps only `title`, `description`, `inputSchema`, `outputSchema`, `annotations`, prefixes names with `<server>__`, leaves out invalid or too long names and later duplicates, and returns one page.

- **Failures:** a failing server is retried once; if it fails again or exceeds a limit, it is left out, so one broken server does not hide the others. Each server left out produces a record with tool `tools/list` in the run view. Consequence: the harness usually lists once per session and `listChanged` is false, so a server missing at session start stays missing for that session (for a chat kept alive, up to its keep-alive). The run view shows why.
- **Deadline:** 15 s per upstream list, 20 s for the whole answer, so the harness's own MCP list timeout is not hit first. Spike 1.2 records the harnesses' timeouts and how often they call `tools/list`; the values are adjusted there if needed.
- **Cache:** the aggregated list of a run is reused for 60 s per gateway pod. If a harness lists per turn, or any process in the sandbox calls `tools/list` in a loop, the upstreams still see at most one list per minute, run and pod. The cache entry is keyed by the run's enabled server ids and their `config_version`s, so any change to them drops it and a disabled server never appears in a list served after the change.
- **Upstream handshake:** `initialize`, its successful response, `notifications/initialized`, then the request; the same after an expired session.

`tools/call` checks only that the prefix names a server enabled for the run (removals apply at once), then forwards the rest of the name. Whether the tool exists is the upstream's answer. The per-tool check comes with the catalog's filters.

### D6: Names, stateless sessions, cancellation

- Tool names: `<server>__<tool>`. Server names match `^[a-z](-?[a-z0-9])*$` with at most 20 characters, so they contain no `_` and the split at the first `__` is unambiguous. The relay is registered as `ext`, so Claude Code shows `mcp__ext__context7__get-library-docs`. Anthropic tool names allow 64 characters; longer tools are left out. Server names are immutable, because a rename would change tool names in running sessions and transcripts.
- `Mcp-Session-Id` is `df-<run id>-<random>`. A gateway pod accepts it when the run id equals the source pod's run. The downstream session holds no state on the gateway: `tools/list` and `tools/call` are answered from the snapshot by pod IP. So every gateway replica serves every request and no session affinity is needed. The pod IP authenticates, as for `/inference`.
- Upstream sessions are state, but per gateway pod: opened on first use, closed when the run leaves the snapshot, when `config_version` changes, or after 10 min idle.
- Cancellation: if the client closes the connection of a call, the pod cancels the upstream request (outcome `cancelled`). This deliberately departs from Streamable HTTP's "disconnection SHOULD NOT be interpreted as cancellation": the endpoint offers no resumable stream, so a result whose connection is gone can never be delivered. `notifications/cancelled` may reach another pod, which ignores it. The concurrency limit (8) is per run and gateway pod.
- Enablement changes apply at once, in both directions: the gateway checks the run's current enabled servers on every call and list. A server enabled during a run is callable at once and appears in the next `tools/list`; a harness that does not list again (`listChanged` is false) does not see it until its next session, and a run whose `Assign` had no endpoint has no `ext` until its next run. Keeping each run's start-time server set and intersecting it was considered (CodeRabbit review) and rejected: enabling a server for a project is the admin's decision for that project, so letting a running run use it grants nothing beyond that decision, and the start-time set would need its own storage per run.

### D7: Contracts

`darkfactory.gateway.v1` (final numbers assigned at implementation, task 2.1):

```proto
message Snapshot {
  // ... existing fields 1-4 ...
  // MCP servers enabled for at least one active run, keyed by server id.
  // Final number assigned when the change is implemented.
  map<string, McpServer> mcp_servers = <n1>;
}

message SandboxEntry {
  // ... existing fields 1-10 ...
  // Ids of the MCP servers enabled for the run's project.
  // Final number assigned when the change is implemented.
  repeated string mcp_server_ids = <n2>;
}

message McpServer {
  string id = 1;
  string name = 2;
  string url = 3;                // may contain path and query parameters
  string credential_ref = 4;     // key into Snapshot.provider_credentials
  // How the credential is sent; exactly one is set.
  oneof credential_scheme {
    McpBearerScheme bearer = 5;  // Authorization: Bearer <value>
    McpHeaderScheme header = 6;  // <name>: <prefix><value>
    McpQueryScheme query = 7;    // ?<parameter>=<value>, URL-encoded
  }
  bool private_network = 8;      // skip the private-address check
  uint64 config_version = 9;     // upstream sessions end when it changes
}

message McpBearerScheme {}
message McpHeaderScheme { string name = 1; string prefix = 2; }
message McpQueryScheme { string parameter = 1; }

service GatewayMcp {
  rpc Report(McpCallBatch) returns (ReportMcpCallsResponse);
}

message McpCallBatch { string gateway_pod = 1; repeated McpCallRecord records = 2; }
message McpCallRecord {
  string call_id = 1;            // UUIDv7, idempotency key
  string run_id = 2;
  google.protobuf.Timestamp started_at = 3;
  string server_name = 4;
  string tool = 5;
  McpCallOutcome outcome = 6;
  uint64 duration_ms = 7;
  uint64 request_bytes = 8;
  uint64 response_bytes = 9;
}
enum McpCallOutcome {
  MCP_CALL_OUTCOME_UNSPECIFIED = 0;
  MCP_CALL_OUTCOME_OK = 1;
  MCP_CALL_OUTCOME_TOOL_ERROR = 2;
  MCP_CALL_OUTCOME_REJECTED = 3;
  MCP_CALL_OUTCOME_UPSTREAM_ERROR = 4;
  MCP_CALL_OUTCOME_TIMEOUT = 5;
  MCP_CALL_OUTCOME_TOO_LARGE = 6;
  MCP_CALL_OUTCOME_CANCELLED = 7;
}
message ReportMcpCallsResponse {}
```

The scheme is a `oneof` (Peter Kurfer's review) instead of the inference route's flat `CredentialScheme` enum plus `credential_header` / `credential_prefix` fields: each scheme carries only its own fields, and an invalid combination cannot be expressed. The inference route keeps its enum in this change. Credential values reuse `Snapshot.provider_credentials` (keyed by credential reference id, already generic); its comment becomes "inference and MCP credentials".

**URLs with query parameters.** `UpstreamUrl::parse` rejects a query, which is right for inference and Git. The MCP client gets its own URL type that keeps path and query, adds a `query` credential as the last parameter, and is never logged or returned with its query (its `Debug` prints scheme, host and path only). Some servers need this: PenPot's MCP server, for example, takes a JWT as a query parameter. Without tool definitions in the snapshot, its size grows by a few hundred bytes per server.

`darkfactory.agent.v1`: `AcpHarness.mcp_endpoint = <n3>` (string, empty = none; final number assigned at implementation). It sits on `AcpHarness` because registering MCP servers is an ACP session concern.

### D8: Runtime: a stdio relay instead of HTTP in the harness

ACP requires every agent to support stdio MCP servers; HTTP MCP is optional (`mcpCapabilities.http`). Registering the gateway as an HTTP server would depend on each harness (Claude adapter, OpenCode, later "Generic ACP" agents) and on its CA and proxy handling. So the runtime registers a second stdio server:

- `session/new` and `session/load` list `factory` (as today) and `ext`: `/opt/factory/factory-agent mcp-relay --url <mcp_endpoint> --ca <INSTANCE_CA_PATH>`.
- `mcp-relay` is a transport bridge, not an MCP server: each JSON-RPC line from stdin becomes a `POST` to the endpoint (with `Mcp-Session-Id` and `MCP-Protocol-Version` once known), each answer is re-serialized as one compact JSON line on stdout. It parses only what the transport needs: the session header, and the request id to answer with a JSON-RPC error when the gateway is unreachable or answers with an HTTP error status. Accepted notifications (202) write nothing. Requests run concurrently, one writer task serializes stdout. It exits on stdin EOF and refuses to start without a readable CA. It connects directly (ignores `HTTPS_PROXY`), trusts only the instance CA, sends no credential and opens no listener. It needs no MCP library.
- A separate server, not tools mixed into `factory`: the transcript keeps telling factory tools (`mcp__factory__*`) and third-party tools apart; factory calls keep their gRPC path with retries on reconnect; third-party calls go only to the gateway, so the control plane stays out of that data path (ADR-0032, alternatives).
- The relay talks to the gateway itself instead of through the runtime's Unix socket: the identity is the pod IP either way, and a hop through the runtime would add code without a gain.

Iteration 4 decision 1 holds in substance (one aggregating gateway endpoint, passed in `session/new`); only the form changes (a stdio relay instead of an HTTP server entry). `iteration-4.md` notes this.

Alternative: HTTP MCP server entry in `session/new` (the first version of this proposal): needs `mcpCapabilities.http` in every harness, a fallback when missing, and harness-specific spikes.

### D9: Call records

The gateway reuses the usage reporter's pattern (bounded queue of 10,000, batches of 100, flush every 2 s, back-off retries, oldest dropped and counted in `df.gateway.mcp.calls_dropped`). A server left out of a `tools/list` gives a record with tool `tools/list`, so missing tools are visible in the run view, not only in the gateway log. `df-gateway-control` stores them in `mcp_calls`; records of unknown runs are dropped and counted, as for usage. The run view gets a "Tool calls (MCP)" section. Arguments and results are not stored: the transcript has them as harness tool calls, truncated at 64 KiB like every tool call.

### D10: UI and authorization

- Settings area: "MCP servers" page next to "Model providers": list and form (name, URL with the note that it is not secret, credential reference select, scheme `bearer` / `header` / `query` with the same preview as model providers, "Private network" switch with a one-line explanation, the write-tools warning).
- Project settings: tab "MCP servers" with one checkbox per server, shown to admins only, like "Integrations".
- Run view: "Tool calls (MCP)" table, for every board member.
- **Admins only** for the MCP servers page (`require_board_member` + `require_admin`) and the project tab (`Editing::AsAdmin`): both decide what a shared credential reaches, like the webhook secret and the lifecycle subscriptions that already need `admin`. With `iter4-permissions`, both map to `credential:manage`; only the route guards change.

### D11: Repository MCP configurations

This change does not touch MCP servers a repository defines for its harness (Claude Code's `.mcp.json`, the `mcp` section of `opencode.json`). Nothing in the runtime or the generated harness configuration enables or disables them today, so each harness applies its own rule:

- **Claude Code** asks the user to approve project-scoped `.mcp.json` servers before it starts them, unless its settings approve them (`enableAllProjectMcpServers`, `enabledMcpjsonServers`). Under the ACP adapter there is no interactive prompt, and the runtime auto-approves ACP permission requests; whether the adapter loads, skips or auto-approves `.mcp.json` servers is not verified (spike 1.2).
- **OpenCode** reads `mcp` entries of the repository's `opencode.json` as configuration, without a separate approval; the generated configuration does not override `mcp` (spike 1.2 confirms).
- Either way, a repository server gets no credential from the factory: a stdio server runs in the sandbox under Landlock, and a remote one goes through the egress proxy and needs an allowlist entry.

The planned `repository-mcp-server` approval (`iter4-mcp-catalog`, decision 8) needs the factory to decide this: block repository servers by default (Claude Code: settings without project MCP approval or with `enabledMcpjsonServers: []`; OpenCode: `enabled: false` per entry in the generated configuration) and enable approved entries. Spike 1.2 records the current behavior so that the catalog change starts from facts. If a harness loads repository servers unasked today, that is a gap to close in the catalog change or earlier, not by this change.

## Security

- **Untrusted descriptions and results.** In this slice, tool descriptions come live from the upstream and reach the model unreviewed (tool poisoning), and results pass unchanged (prompt injection). The gateway limits what an upstream can do beyond text: no sampling, elicitation or roots, size and time limits, only five tool fields served. Review and pinning of descriptions come with `iter4-mcp-catalog`. Until then: register only servers whose operator you trust.
- **Write tools bypass the factory's controls.** A write tool acts directly on the upstream system. For a forge server (forgejo-mcp) that means commits, merges, file changes and issues **without** the Git proxy's push checks, secret rules and task-branch rules, and without the `create-issue` approval of `chat-issue-tools`; in chats, too, nothing asks a human. Any tool argument also leaves to the upstream, read-only tools included. **Rule for this slice** (docs and form warning): the credential's scope is the permission. Use read-only tokens where possible; register no forge server with a write-capable token until filters and "ask" exist. One technical guard: an MCP server cannot use a credential reference that a project uses as its forge credential or a model provider uses (and the other way round), so the factory's own forge token cannot be selected by mistake. A separate write-capable token remains possible; the warning covers it. Do not enable a server that reads private data together with one that writes to a public place in the same project: an injected result can drive the other server's tools.
- **Credentials.** In the control plane and in gateway memory only. The gateway builds upstream requests itself and forwards no sandbox header. Nothing from upstream response headers goes back. The relay carries no credential.
- **Query credentials** (scheme `query`) are weaker than header credentials outside our control: the upstream, and any proxy in front of it, may write full URLs to its access logs. On our side the URL's query is never logged, never put into an error message and never returned; redirects are not followed, so the URL cannot leak to a third host. The form recommends `bearer` or `header` when the server supports them.
- **The URL is not secret.** It is stored in the database and shown in the UI. A token pasted into the URL instead of the credential reference would be exposed; the form says so.
- **One credential per server per instance.** The upstream sees all projects' calls under one identity. Any project that enables a server reaches everything the credential reaches, and the upstream's audit log cannot tell projects apart. One run can use up the credential's rate limit for all projects. Per-project credentials and rate limits: backlog.
- **Any process in the sandbox** can call `/mcp`, as with `/inference`: the trust boundary is the sandbox, not the harness.
- **SSRF.** D3: public servers checked, private ones by explicit switch, no redirects.
- **Limits.** 1 MiB request, 4 MiB result, 1,000 tools and 4 MiB per upstream list, 10 s connect, 15 s per upstream list and 20 s per `tools/list`, list cache 60 s, 120 s call, 8 concurrent calls per run and gateway pod.
- **Client capabilities** the harness sends in `initialize` are ignored; the gateway never sends requests to the harness.
- **Permission prompts.** The runtime auto-approves ACP permission requests, so harnesses run `ext` tools without asking, as all tools today.

## Risks / Trade-offs

- [Upstream servers use protocol versions or SSE behaviors the thin client does not handle] → spike 1.1 against Context7 and the fake MCP server; tests cover JSON and SSE answers.
- [Large tool sets fill the model context] → per-server limit of 1,000 tools; filters with the catalog; `search_tools` / `load_server` in the backlog.
- [A harness ignores `ext` because a repository configuration defines a server with that name] → the same situation as `factory` today; spike 1.2 checks both harnesses.
- [A harness's tool list call takes long because an upstream is slow] → 15 s per upstream list, in parallel, 20 s overall; a slow server is left out with a record in the run view.
- [A server missing at session start stays missing for the session] → one retry, a `tools/list` record in the run view; a new run lists again.
- [Gateway on the path of every MCP call] → same as inference (ADR-0050); a gateway outage already stops agents.

## Migration Plan

Additive: new tables, new proto fields, new endpoint, new runtime subcommand. A run without enabled servers behaves as before. Rollback: disable all servers (no `Assign.mcp_endpoint`); the migration has a down path that drops the new tables. Release notes name the new page and the docs.

## Decisions to confirm (open questions with recommendation)

Taken on 2026-11-25 (Peter Kurfer): gateway first with all tools forwarded (D1); the "Private network" switch instead of a fixed private-address check (D3); the stdio relay through the runtime instead of an HTTP server in the harness (D8).

| # | Question | Recommendation |
|---|---|---|
| Q1 | Enable servers per project or per persona, opt-in or opt-out? | Per project, opt-in (D1, amends ADR-0032). A server is an external system with its own data; which projects may reach it is the blast-radius decision, and it matches the egress rules. Persona grants later intersect with it. |
| Q2 | Write tools until the catalog? | No block of write tools; the form warning, the documented rule (read-only tokens, no forge write tokens), and the guard that the factory's own forge and model credentials cannot be used for MCP servers. |
| Q3 | Store call arguments or results? | No. Size and outcome only; the transcript has the payloads. |
| Q4 | Who registers servers and enables them per project? | Users with the existing `admin` role, for both (D10); `credential:manage` after `iter4-permissions`. |
| Q5 | One credential per server per instance? | Yes, with the consequences documented. Per-project credentials and rate limits to the backlog. |
| Q6 | Apply the private-address check to inference and Git upstreams too? | No, not in this change; in-cluster model routers are a real case there. |
| Q7 | Limits fixed or configurable? | Fixed constants (as for inference). |
| Q8 | Server name `ext` and separator `__`? | Yes; short because of the 64-character tool name limit. |
| Q9 | PenPot designs for the three screens? | Not required; they follow the model provider form and the egress tab. |

## Open Questions

- Context7's credential header (`CONTEXT7_API_KEY` or `Authorization: Bearer`) for the docs example: spike 1.3, no change to the design.
