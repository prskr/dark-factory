# Design: iter4-mcp-gateway

## Context

ADR-0032 and `design/egress.md` §10 describe the target: one aggregating MCP endpoint in the gateway, tool catalogs in the control plane, grants per persona. Iteration 4 (decisions 1, 7, 8) fixes HTTP upstreams with static credentials. Iteration 4 also orders `iter4-permissions` → `iter4-mcp-catalog` → `iter4-mcp-gateway`. This change reverses the last two and cuts both down to one working path (D1).

What exists today (observed in the code):

- **Gateway.** `/inference` shares the Git port (8443) and its TLS listener (`df-gateway/src/git/server.rs`, hyper, HTTP/1.1). Runs are found by source pod IP (`Policy::sandbox`). Credentials come in the `PolicyWatch` snapshot (`Snapshot.provider_credentials`, keyed by credential reference id) and are injected in `inference/headers.rs`. Usage records go through a bounded, batched reporter (`inference/report.rs`, `GatewayUsage.Report`).
- **Upstream connections.** `UpstreamConnector` (`df-gateway/src/upstream.rs`) is shared by Git, mirrors and inference. It connects directly with `TcpStream::connect` and does **not** apply the proxy's private-destination check (`resolve::BlockedRanges`, used only by `proxy::Proxy`). `UpstreamUrl::parse` accepts `https` only (`http` with `network.allowPlainHttp`) and rejects user information, query and fragment.
- **Credentials.** `credential_refs` point at a Kubernetes Secret name and key; `SecretStore` resolves them at use time (`credential-resolution`). Model providers reference them with a scheme (`x-api-key`, `bearer`, `header`, `claude-subscription`).
- **Runtime.** `factory-agent/src/harness.rs` `setup_session()` sends `initialize` with empty client capabilities, ignores the agent's capabilities, and passes only the stdio `factory` server to `session/new` and `session/load`. The inference URL reaches the harness only as env in `AcpHarness.env`.
- **`rmcp` 3.9** is a workspace dependency with features `client`, `server`, `transport-async-rw` (stdio bridge in `factory-agent`, client in `df-harness`). No Streamable HTTP transport is enabled.
- **Authorization.** Settings pages (model providers, credentials, personas, projects) sit behind `require_board_member`. There is no `admin` role or tool permission yet.

## Goals / Non-Goals

**Goals:**
- One end-to-end path: register a server → pin tools → enable for a project → run calls a tool through the gateway → call record in the run view.
- Credentials never in the sandbox, the transcript or logs.
- The model only ever sees tool definitions a human has reviewed.
- Every piece is a subset of the ADR-0032 target, so `iter4-mcp-catalog` and the permission model extend it instead of replacing it.

**Non-Goals:** (proposal "Out of scope" has the full list and roadmap targets)
- Automatic or periodic discovery, catalog history, glob filters: `iter4-mcp-catalog` (roadmap iteration 4).
- Persona grants and per-project single-tool restrictions: per-persona tool permissions, roadmap iteration 4 "Full permission model (ADR-0011)".
- `search_tools` / `load_server`: backlog row "MCP meta-tools for large tool sets" (added by this change).
- Credentials per project or persona for the same server: backlog row "MCP server credentials per project" (added by this change).
- An in-cluster upstream opt-in (stdio → HTTP bridge): `iter4-mcp-catalog` with the bridge, roadmap iteration 4.
- Private-destination check for the inference and Git upstreams: rejected in this change (see Q8); it is a separate question for model providers that live in the cluster.
- Gateway HA session affinity: not needed, sessions are stateless (D5).

## Decisions

### D1: The slice: gateway first, a pinned list as the first catalog

`iter4-mcp-catalog` before `iter4-mcp-gateway` means the catalog, filters, grants and approvals must be built before any agent calls any tool. A catalog without a gateway is not testable end to end, and its UI decisions (filters, flags, grant dialogs) are easier to make once real servers are in use.

The slice keeps the one catalog property that matters for security, "upstream changes cannot silently reach the model", in its smallest form: the human fetches, reads and selects; the definitions are pinned; the gateway serves the pinned definitions. Periodic discovery later only adds "a refetch happens automatically and flags differences"; the pinned selection stays the source of a run's tool set.

Alternatives: catalog first (plan of iteration 4): larger first change, nothing usable until both are done. Gateway first with the live upstream tool list and a name filter only: a server can change a description after review and poison every running agent.

### D2: Data model

| Table | Columns (main) | Notes |
|---|---|---|
| `mcp_servers` | `id`, `name` UNIQUE, `url`, `credential_ref_id` → `credential_refs`, `credential_scheme` CHECK `bearer`\|`header`, `credential_header`, `credential_prefix`, timestamps | Name immutable (D5). |
| `mcp_server_tools` | `server_id` → `mcp_servers` ON DELETE CASCADE, `name`, `definition` JSONB (the tool object as fetched), `selected` bool, `fetched_at`; PK (`server_id`, `name`) | Every tool of the last saved fetch. `selected` rows are the pinned tools; unselected rows let the next fetch tell "new" from "seen before". |
| `mcp_server_fetches` | `server_id` PK → `mcp_servers` ON DELETE CASCADE, `tools` JSONB, `fetched_at` | The last fetch shown to the board member. Saving applies exactly this list, never a new upstream answer. Replaced by every fetch. |
| `project_mcp_servers` | `project_id`, `server_id` → `mcp_servers` (RESTRICT); PK both | Enablement. Delete refused while rows exist (spec "Servers in use cannot be deleted"). |
| `mcp_calls` | `call_id` UUID PK, `run_id` → `runs` ON DELETE CASCADE, `server_name`, `tool`, `outcome`, `started_at`, `duration_ms`, `request_bytes`, `response_bytes` | `server_name` instead of an FK, so records survive a server deletion. PK makes retried reports idempotent, as `model_usage.request_id`. |

"Changed" compares the canonical JSON of `title`, `description`, `inputSchema`, `outputSchema` and `annotations`. Queries live in a new `df-db` module `mcp_servers`.

### D3: "Fetch tools" runs in the control plane, with the gateway's client

The fetch needs the credential, an MCP client, the destination check and the limits. The gateway's MCP client (D6) has all of it. `df-gateway-control` may already depend on `df-gateway` (`docs/architecture.md`), so the control plane calls the same client in-process: `initialize`, `tools/list` with all pages (max. 4 MiB in total, 1,000 tools), `DELETE` of the session. `df-web` declares a single-method trait `McpToolLister` (the consumer, AGENTS.md "Traits over closures"); the binary wires the `df-gateway-control` implementation. The architecture table gets the new use.

Alternative: a new RPC on the gateway's internal port so a gateway pod fetches. Needs pod selection (as the log relay) for no gain: the control plane already has outbound internet access for forge APIs.

### D4: Endpoint transport: own thin layer, `rmcp` for types

The endpoint is a small JSON-RPC server: `initialize`, `ping`, `tools/list`, `tools/call`, two notifications, everything else `-32601`. Responses are `application/json` (Streamable HTTP lets a server answer a POST with JSON instead of SSE); there is no server-initiated message, so `GET` is 405.

The upstream side is a small client: `POST` JSON-RPC, accept a JSON or an SSE response, read the SSE stream until the response with the request's id, answer server requests on the stream with `-32601`, drop notifications, keep `Mcp-Session-Id`, send `MCP-Protocol-Version`.

Both run on what the gateway already has: the hyper server of the Git port and `UpstreamConnector`, so TLS, timeouts, `LimitedBody` and the destination check (D6) stay in one place. `rmcp`'s model types (`Tool`, `CallToolResult`, `JsonRpcMessage`) give the wire format. Its Streamable HTTP transports bring axum on the server side and reqwest on the client side, a second HTTP stack each, with their own connection handling that would bypass our checks. Spike 1.3 confirms the model types are usable alone and that the protocol versions of Claude Code and OpenCode are covered; if `rmcp`'s transport can take our connector with little code, use it.

### D5: Names and stateless sessions

- Tool names: `<server>__<tool>`. Server names are `[a-z][a-z0-9-]{0,19}` without `__`, so the split at the first `__` is unambiguous. The gateway server is registered as `ext` in the harness, so Claude Code shows `mcp__ext__context7__get-library-docs`. Anthropic tool names allow 64 characters; the selection disables tools whose full name would be longer (spec "Tool names agents can use"). Server names are immutable, because a rename would change tool names in running sessions and transcripts.
- `Mcp-Session-Id` is `df-<run id>-<random>`. A gateway pod accepts it when the run id equals the source pod's run. The session holds no state on the gateway: `tools/list` and `tools/call` are answered from the snapshot by pod IP. So every gateway replica can serve every request and no session affinity is needed. The session id is not an authenticator; the pod IP is, as for `/inference`.
- Upstream sessions are state, but per gateway pod: each pod opens its own session per run and server on first use, closes them when the run leaves its snapshot, and drops them after 10 min idle. A second pod opening a second upstream session is harmless.
- Tool set changes apply to calls at once (each call is checked against the current snapshot). The listed tools change at the next session, since `listChanged` is false (proposal, rejected item).

### D6: Upstream connections and the destination check

The MCP client uses `UpstreamConnector` with a resolver step in front: the gateway resolves the host with `SystemResolver`, refuses the address if `BlockedRanges` covers it (private, link-local, loopback, cluster ranges), and connects to the checked address with the original host for SNI. This follows §7 of `design/egress.md`. It matters more here than for inference: a third party controls the server's DNS, and a tool call is an arbitrary POST body chosen by the agent. Without the check, a rebinding server turns the gateway into a client for in-cluster services (the control plane's internal port, the database) with the responses returned to the agent.

No `http-host` allowlist entry is needed: registering the server is the allow decision (as for model providers since ADR-0050), and the enablement per project is the scope. The gateway's own network policy already allows outbound traffic to the internet.

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
  // Ids of the MCP servers enabled for the run's project that have pinned tools.
  // Final number assigned when the change is implemented.
  repeated string mcp_server_ids = <n2>;
}

message McpServer {
  string id = 1;
  string name = 2;
  string url = 3;
  string credential_ref = 4;     // key into Snapshot.provider_credentials
  CredentialScheme scheme = 5;   // BEARER or HEADER only
  string credential_header = 6;
  string credential_prefix = 7;
  repeated McpTool tools = 8;    // pinned tools only
}

message McpTool {
  string name = 1;               // upstream name, without prefix
  string definition_json = 2;    // pinned tool object (title, description, schemas, annotations)
}

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
  MCP_CALL_OUTCOME_UNSPECIFIED = 0; OK = 1; TOOL_ERROR = 2; REJECTED = 3;
  UPSTREAM_ERROR = 4; TIMEOUT = 5; TOO_LARGE = 6;
}
message ReportMcpCallsResponse {}
```

Credential values reuse `Snapshot.provider_credentials` (keyed by credential reference id, already generic); its comment is updated to "inference and MCP credentials". The pinned definition is carried as JSON text, so the gateway serves it byte for byte apart from the prefixed name, and the contract does not mirror the MCP tool schema.

`darkfactory.agent.v1`: `AcpHarness.mcp_endpoint = <n3>` (string, empty = none; final number assigned at implementation). It sits next to the inference env on `AcpHarness` because registering an MCP server is an ACP session concern.

### D8: Call records

The gateway reuses the usage reporter's pattern (bounded queue of 10,000, batches of 100, flush every 2 s, back-off retries, oldest dropped and counted in `df.gateway.mcp.calls_dropped`). `df-gateway-control` stores them in `mcp_calls`; records of unknown runs are dropped and counted, as for usage. The run view gets a "Tool calls (MCP)" section listing them. Arguments and results are not stored: the transcript already holds them as ACP tool call events, and a second copy would be a second place for secrets that upstream results may contain.

### D9: Runtime

`setup_session()` keeps the `initialize` response and reads `agentCapabilities.mcpCapabilities.http`. With an endpoint in `Assign` and the capability, it adds `McpServer::Http { name: "ext", url, headers: [] }` to `session/new` and `session/load`. Without the capability, it emits `harness.mcp_http_unsupported` once and continues. No header, token or credential is passed: identity is the pod IP. The harness already has the CA variables and reaches the gateway host directly (same host and port as `/inference`).

### D10: UI

- Settings area: "MCP servers" page next to "Model providers": list, form (name, URL, credential reference select, scheme with the same preview as model providers), "Fetch tools" with the tool table (select box, name, title, "may write" mark, new/changed/removed badges, expandable description and schema), "Save selection". Descriptions are rendered as plain text, never as Markdown or HTML (they are untrusted).
- Project settings: tab "MCP servers" with one checkbox per server and the number of pinned tools.
- Run view: "Tool calls (MCP)" table.

The screens reuse the model provider form and the egress tab patterns; a PenPot round is not needed for this slice unless the reviewer asks for it (Q11).

### D11: Who may do it

Board members, through `require_board_member`, like model providers and credentials. When `iter4-permissions` adds roles: registering servers and pinning tools map to `credential:manage` / `agent:configure` (the `admin` role), enabling per project to the project's configuration permission. Only the route guards change.

## Security

- **Untrusted descriptions (tool poisoning).** The model sees only pinned definitions a board member has read. A server that changes a description after review changes nothing until a human refetches, sees "changed" and saves. The UI shows descriptions as plain text, in full, so hidden instructions are visible. Not covered: a malicious description that the reviewer misses.
- **Untrusted results (prompt injection).** Results pass unchanged; the gateway cannot tell data from instructions. Limits: the size cap, no sampling, no elicitation, no roots (an upstream cannot make the model act or ask the human), and everything else stays as before (egress allowlist, push checks, approvals). Consequence: an injected result can make the agent call other enabled tools. Enabling a server that reads private data and one that can write to the public internet in the same project creates an exfiltration path; the docs say so.
- **Credentials.** In the control plane and in gateway memory only. The gateway builds upstream requests itself and forwards no sandbox header, so a harness cannot override or read the credential. Nothing from the upstream's response headers goes back to the harness.
- **One credential per server per instance.** The upstream sees all projects' calls under one identity. Any project that enables a server can reach everything that credential can reach (for example all repositories a forgejo token can read), and the upstream's own audit log cannot tell projects apart. Operators scope the token to what every enabling project may see. Per-project credentials: backlog.
- **Write tools as an exfiltration channel.** Any tool argument leaves the cluster to the upstream, read-only tools included (a search query can carry data). Default: nothing is selected; tools without `readOnlyHint: true` are marked "may write"; annotations never select anything. There is no approval per call in this slice (the `tool-access` approval and "ask" come later), so a selected write tool runs without a human, also in chats, unlike `create_issue` (`chat-issue-tools`). Recommendation in the docs: select write tools only for servers whose write target is internal.
- **Any process in the sandbox** can call `/mcp`, as with `/inference`: the trust boundary is the sandbox, not the harness.
- **SSRF.** D6.
- **Limits.** 1 MiB request, 4 MiB result, 10 s connect, 120 s call, 8 concurrent calls per run and gateway pod; fetch at most 4 MiB and 1,000 tools.
- **Permission prompts.** The runtime auto-approves ACP permission requests, so Claude Code runs `ext` tools without asking. That is the existing behavior for all tools.

## Risks / Trade-offs

- [The Claude ACP adapter or OpenCode does not advertise `mcpCapabilities.http`, or ignores HTTP servers] → spikes 1.1 and 1.2 before any runtime work; fallback for OpenCode: a `mcp.ext` remote entry in `OPENCODE_CONFIG_CONTENT`.
- [Upstream servers use protocol versions or SSE behaviors the thin client does not handle] → spike 1.3 against Context7 and the fake MCP server; the fake server in tests covers JSON and SSE answers.
- [Pinned schemas go stale; calls fail after an upstream change] → the call returns the upstream's error to the agent; the board member refetches. Automatic detection comes with `iter4-mcp-catalog`.
- [Large pinned sets fill the model context] → humans select tools; `search_tools` / `load_server` in the backlog.
- [A repository's `.mcp.json` or `opencode.json` defines servers the harness connects to directly] → those connections go through the egress proxy and need allowlist entries; decision 8 and `iter4-mcp-catalog` handle `.mcp.json`. Spikes 1.1/1.2 check that a repository server named `ext` cannot replace the gateway's.
- [Gateway on the path of every MCP call] → same as inference (ADR-0050); a gateway outage already stops agents.

## Migration Plan

Additive: new tables, new proto fields, new endpoint. A run without enabled servers behaves as before. Rollback: disable all servers (no `Assign.mcp_endpoint`, gateway answers 403); the migration has a down path that drops the new tables. Release notes name the new page and the docs.

## Decisions to confirm (open questions with recommendation)

| # | Question | Recommendation |
|---|---|---|
| Q1 | Ship this slice before `iter4-mcp-catalog`, reversing the iteration 4 order? | Yes (D1). `iter4-mcp-catalog` becomes the follow-up that adds discovery, flags, filters, `tool-access` and `.mcp.json`. |
| Q2 | Enable servers per project or per persona? | Per project. A server is an external system with its own data; which projects may reach it is the blast-radius decision, and it matches the egress rules (project scope). Persona grants later intersect with it (ADR-0032). |
| Q3 | Default for write tools? | Nothing preselected, "may write" mark, no extra gate in this slice. A switch based on annotations is weak because annotations are untrusted; the human selection is the gate. |
| Q4 | Store call arguments or results? | No. Size and outcome only; the transcript has the payloads. |
| Q5 | Who registers servers? | Board members now; `admin` (`credential:manage`) after `iter4-permissions`. |
| Q6 | One credential per server per instance? | Yes, with the consequence documented. Per-project credentials to the backlog. |
| Q7 | Require `http-host` allowlist entries for upstream hosts? | No. Registration is the allow decision (as for model providers). |
| Q8 | Private-destination check for MCP upstreams, although inference and Git skip it? | Yes for MCP (D6). Do not change inference or Git in this change: in-cluster model routers are a real case there and paths are fixed. |
| Q9 | Limits fixed or configurable? | Fixed constants (as for inference). Configuration when a server needs more. |
| Q10 | Harness-visible server name `ext` and separator `__`? | Yes; short because of the 64-character tool name limit. |
| Q11 | PenPot designs for the three screens? | Not required; they follow the model provider form and the egress tab. Ask if the reviewer wants them. |

## Open Questions

- Context7's credential header (`CONTEXT7_API_KEY` or `Authorization: Bearer`) for the docs example: spike 1.4, no change to the design.
