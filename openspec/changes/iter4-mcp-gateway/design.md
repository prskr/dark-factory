# Design: iter4-mcp-gateway

## Context

ADR-0032 and `design/egress.md` §10 describe the target: one aggregating MCP endpoint in the gateway, tool catalogs in the control plane, grants per persona. Iteration 4 (decisions 1, 7, 8) fixes HTTP upstreams with static credentials. Iteration 4 also orders `iter4-permissions` → `iter4-mcp-catalog` → `iter4-mcp-gateway`. This change reverses the last two and cuts both down to one working path (D1).

What exists today (observed in the code):

- **Gateway.** `/inference` shares the Git port (8443) and its TLS listener (`df-gateway/src/git/server.rs`, hyper, HTTP/1.1). Runs are found by source pod IP (`Policy::sandbox`). Credentials come in the `PolicyWatch` snapshot (`Snapshot.provider_credentials`, keyed by credential reference id) and are injected in `inference/headers.rs`. Usage records go through a bounded, batched reporter (`inference/report.rs`, `GatewayUsage.Report`). The gateway's `PolicyWatch` client uses tonic's default 4 MiB decode limit; only `df-agent-channel/src/server.rs` raises its limit.
- **Upstream connections.** `UpstreamConnector` (`df-gateway/src/upstream.rs`) is shared by Git, mirrors and inference. It connects directly with `TcpStream::connect` and does **not** apply the proxy's private-destination check (`resolve::BlockedRanges`, used only by `proxy::Proxy`, which refuses a name if any resolved address is blocked). `BlockedRanges` covers private, loopback and link-local ranges plus `gateway.blockedCidrs` (default `[]`), so cluster ranges outside RFC 1918 are blocked only if an operator lists them. `UpstreamUrl::parse` accepts `https` only (`http` with `network.allowPlainHttp`) and rejects user information, query and fragment.
- **Credentials.** `credential_refs` point at a Kubernetes Secret name and key; `SecretStore` resolves them at use time (`credential-resolution`). Model providers reference them with a scheme (`x-api-key`, `bearer`, `header`, `claude-subscription`).
- **Runtime.** `factory-agent/src/harness.rs` `setup_session()` sends `initialize` with empty client capabilities, ignores the agent's capabilities, and passes only the stdio `factory` server to `session/new` and `session/load`. The inference URL reaches the harness only as env in `AcpHarness.env`. The transcript marks tool calls `mcp__factory__*` as factory calls, all others as harness calls, and truncates raw input and output at 64 KiB (`transcript.rs`).
- **`rmcp` 3.9** is a workspace dependency with features `client`, `server`, `transport-async-rw` (stdio bridge in `factory-agent`, client in `df-harness`). No Streamable HTTP transport is enabled.
- **Authorization.** Settings pages (model providers, credentials, personas, projects) sit behind `require_board_member`. There is no `admin` role or tool permission yet.

## Goals / Non-Goals

**Goals:**
- One end-to-end path: register a server → pin tools → enable for a project → run calls a tool through the gateway → call record in the run view.
- Credentials never in the sandbox, the transcript or logs.
- The model only ever sees tool definitions a human has reviewed.
- MCP data can never break the gateway's other traffic.
- Every piece is a subset of the ADR-0032 target, so `iter4-mcp-catalog` and the permission model extend it instead of replacing it.

**Non-Goals:** (proposal "Out of scope" has the full list and roadmap targets)
- Automatic or periodic discovery, catalog history, glob filters, persona grants: `iter4-mcp-catalog` (roadmap iteration 4).
- `search_tools` / `load_server`: backlog row "MCP meta-tools …" (added by this change).
- Credentials per project or persona for the same server: backlog row "MCP server credentials per project …" (added by this change).
- Rate limits per run against the shared upstream quota: backlog row "MCP call rate limits per run" (added by this change).
- An in-cluster upstream opt-in (stdio → HTTP bridge): `iter4-mcp-catalog` with the bridge, roadmap iteration 4.
- Private-destination check for the inference and Git upstreams: rejected in this change (Q8).
- Gateway session affinity: not needed, sessions are stateless (D5).

## Decisions

### D1: The slice: gateway first, a pinned list as the first catalog

`iter4-mcp-catalog` before `iter4-mcp-gateway` means the catalog, filters, grants and approvals must be built before any agent calls any tool. A catalog without a gateway is not testable end to end, and its UI decisions (filters, flags, grant dialogs) are easier to make once real servers are in use.

The slice keeps the one catalog property that matters for security, "upstream changes cannot silently reach the model", in its smallest form: the human fetches, reads and selects; the definitions are pinned; the gateway serves the pinned definitions. Periodic discovery later only adds "a refetch happens automatically and flags differences"; the pinned selection stays the source of a run's tool set.

**Amendment of ADR-0032.** ADR-0032 has every enabled tool available to granted personas, and projects can *restrict further* (default allow). This change makes projects **opt in** per server (default deny). The reason is the shared credential: enabling a server for a project gives that project's agents everything the credential reaches. When persona grants come (`iter4-mcp-catalog`), a run's tool set becomes pinned tools ∩ servers enabled for the project ∩ persona grants. Task 8.2 records this in ADR-0032 ("Amended by").

Alternatives: catalog first (plan of iteration 4): larger first change, nothing usable until both are done. Gateway first with the live upstream tool list and a name filter only: a server can change a description after review and poison every running agent.

### D2: Data model

| Table | Columns (main) | Notes |
|---|---|---|
| `mcp_servers` | `id`, `name` UNIQUE, `url`, `credential_ref_id` → `credential_refs` ON DELETE RESTRICT, `credential_scheme` CHECK `bearer`\|`header`, `credential_header`, `credential_prefix`, `config_version`, timestamps | Name immutable (D5). `config_version` increases on every edit; gateways end upstream sessions when it changes. A URL change unselects all tools and deletes the stored fetch (spec "Editing a server"). |
| `mcp_server_tools` | `server_id` → `mcp_servers` ON DELETE CASCADE, `name`, `definition` JSONB, `selected` bool, `fetched_at`; PK (`server_id`, `name`) | Every tool of the last saved fetch. `selected` rows are the pinned tools; unselected rows let the next fetch tell "new" from "seen before". `definition` holds only `title`, `description`, `inputSchema`, `outputSchema` and `annotations`; other fields of the fetched object (`_meta`, `icons`, future ones) are dropped, so what is compared is exactly what is served. |
| `mcp_server_fetches` | `id` UUID PK, `server_id` UNIQUE → `mcp_servers` ON DELETE CASCADE, `tools` JSONB, `fetched_at` | The last fetch shown. A new fetch replaces the row with a new `id`. The form posts the `id` it showed; a save with another `id` is refused (stale fetch). Saving applies exactly this list, never a new upstream answer. |
| `project_mcp_servers` | `project_id`, `server_id` → `mcp_servers` (RESTRICT); PK both | Enablement. Delete refused while rows exist. |
| `mcp_calls` | `call_id` UUID PK, `run_id` → `runs` ON DELETE CASCADE, `server_name`, `tool`, `outcome`, `started_at`, `duration_ms`, `request_bytes`, `response_bytes` | `server_name` instead of an FK, so records survive a server deletion. PK makes retried reports idempotent, as `model_usage.request_id`. |

"Changed" compares the canonical JSON of the five stored fields. Limits on save: 100 pinned tools and 256 KiB of pinned definitions per server, 2 MiB in the instance (D7). Queries live in a new `df-db` module `mcp_servers`.

### D3: "Fetch tools" runs in the control plane, with the gateway's client

The fetch needs the credential, an MCP client, the destination check and the limits. The gateway's MCP client (D6) has all of it. `df-gateway-control` may already depend on `df-gateway` (`docs/architecture.md`), so the control plane calls the same client in-process: `initialize`, `tools/list` with all pages (max. 4 MiB in total, 1,000 tools, 30 s), `DELETE` of the session. `df-web` declares a single-method trait `McpToolLister` (the consumer, AGENTS.md "Traits over closures"); the binary wires the `df-gateway-control` implementation. The architecture table gets the new use.

Alternative: a new RPC on the gateway's internal port so a gateway pod fetches. Needs pod selection (as the log relay) for no gain: the control plane already has outbound internet access for forge APIs.

### D4: Endpoint transport: own thin layer, `rmcp` for types

The endpoint is a small JSON-RPC server: `initialize`, `ping`, `tools/list`, `tools/call`, two notifications, everything else `-32601`. Responses are `application/json` (Streamable HTTP lets a server answer a POST with JSON instead of SSE); there is no server-initiated message, so `GET` is 405. Batches are refused (MCP 2025-06-18 removed them). The supported protocol versions are those `rmcp` 3.9 models, at least the ones Claude Code and OpenCode send (spike 1.3).

The upstream side is a small client: `POST` JSON-RPC, accept a JSON or an SSE response, read the SSE stream until the response with the request's id, answer server requests on the stream with `-32601`, drop notifications, keep `Mcp-Session-Id`, send `MCP-Protocol-Version`, never follow redirects.

Both run on what the gateway already has: the hyper server of the Git port and `UpstreamConnector`, so TLS, timeouts, `LimitedBody` and the destination check (D6) stay in one place. `rmcp`'s model types (`Tool`, `CallToolResult`, `JsonRpcMessage`) give the wire format. Its Streamable HTTP transports bring axum on the server side and reqwest on the client side, a second HTTP stack each, with their own connection handling that would bypass our checks. Spike 1.3 confirms the model types are usable alone; if `rmcp`'s transport can take our connector with little code, use it.

### D5: Names, stateless sessions, cancellation

- Tool names: `<server>__<tool>`. Server names match `^[a-z](-?[a-z0-9])*$` with at most 20 characters (no `--`, no trailing `-`), so they contain no `_` and the split at the first `__` is unambiguous. The gateway server is registered as `ext` in the harness, so Claude Code shows `mcp__ext__context7__get-library-docs`. Anthropic tool names allow 64 characters; the selection disables tools whose full name would be longer. Server names are immutable, because a rename would change tool names in running sessions and transcripts.
- `Mcp-Session-Id` is `df-<run id>-<random>`. A gateway pod accepts it when the run id equals the source pod's run. The session holds no state on the gateway: `tools/list` and `tools/call` are answered from the snapshot by pod IP. So every gateway replica can serve every request and no session affinity is needed. The session id is not an authenticator; the pod IP is, as for `/inference`. A known sandbox without enabled servers is served with an empty list, so disabling the last server mid-run gives `rejected` records instead of HTTP errors.
- Upstream sessions are state, but per gateway pod: each pod opens its own session per run and server on first use, and closes it when the run leaves its snapshot, when the server's `config_version` changes, or after 10 min idle. A second pod opening a second upstream session is harmless.
- Cancellation: a call belongs to the HTTP request that carries it. If the harness closes that connection, the pod cancels the upstream request (outcome `cancelled`). `notifications/cancelled` arrives as a separate POST and may reach another pod; the pod holding the call acts on it, other pods ignore it. The concurrency limit (8) is therefore per run and gateway pod; with N replicas a run can have up to 8 × N calls in progress.
- Tool set changes: removals apply to calls at once (each call is checked against the current snapshot). Additions appear from the next run: a run whose `Assign` had no endpoint never registers `ext`, and `listChanged` is false. Chats with a long keep-alive (`read-only-workspace-emptydir`) see additions after their pod ends.

### D6: Upstream connections and the destination check

The MCP client uses `UpstreamConnector` with a resolver step in front: the gateway resolves the host with `SystemResolver`, refuses the connection if `BlockedRanges` covers **any** resolved address (as `proxy.rs` does), and connects to a checked address with the original host for SNI. Redirects are never followed, so a 3xx cannot lead to an unchecked address. This follows §7 of `design/egress.md`. It matters more here than for inference: a third party controls the server's DNS, and a tool call is an arbitrary POST body chosen by the agent. Without the check, a rebinding server turns the gateway into a client for in-cluster services (the control plane's internal port, the database) with the responses returned to the agent.

Cluster ranges outside RFC 1918 (for example a `100.64.0.0/10` service CIDR) are refused only if the operator lists them in `gateway.blockedCidrs`; the security page says so.

No `http-host` allowlist entry is needed: registering the server is the allow decision (as for model providers since ADR-0050), and the enablement per project is the scope. The gateway's own network policy already allows outbound traffic to the internet.

### D7: Contracts and snapshot size

Pinned definitions travel in every `PolicyWatch` snapshot, and a gateway that cannot decode a snapshot stops all its traffic. Two measures: pinned definitions are capped on save (256 KiB per server, 2 MiB per instance), and the gateway's `PolicyWatch` client raises its decode limit to 16 MiB, enough for the MCP cap plus the existing routes of a few hundred runs. Each server is carried once in the snapshot, not per run.

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
  CredentialScheme scheme = 5;   // CREDENTIAL_SCHEME_BEARER or CREDENTIAL_SCHEME_HEADER only
  string credential_header = 6;
  string credential_prefix = 7;
  uint64 config_version = 8;     // upstream sessions end when it changes
  repeated McpTool tools = 9;    // pinned tools only
}

message McpTool {
  string name = 1;               // upstream name, without prefix
  string definition_json = 2;    // title, description, inputSchema, outputSchema, annotations
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

Credential values reuse `Snapshot.provider_credentials` (keyed by credential reference id, already generic); its comment is updated to "inference and MCP credentials". The pinned definition is carried as JSON text of the five fields, so the contract does not mirror the MCP tool schema.

`darkfactory.agent.v1`: `AcpHarness.mcp_endpoint = <n3>` (string, empty = none; final number assigned at implementation). It sits next to the inference env on `AcpHarness` because registering an MCP server is an ACP session concern.

### D8: Call records

The gateway reuses the usage reporter's pattern (bounded queue of 10,000, batches of 100, flush every 2 s, back-off retries, oldest dropped and counted in `df.gateway.mcp.calls_dropped`). `df-gateway-control` stores them in `mcp_calls`; records of unknown runs are dropped and counted, as for usage. The run view gets a "Tool calls (MCP)" section listing them.

Arguments and results are not stored in the records. The transcript has them as harness tool calls (origin "harness", not "factory", since only `mcp__factory__*` is classified as factory), with raw input and output truncated at 64 KiB like every tool call. A second copy would be a second place for secrets that upstream results may contain. A separate transcript origin for `ext` calls is not needed for this slice.

### D9: Runtime

`setup_session()` keeps the `initialize` response and reads `agentCapabilities.mcpCapabilities.http`. With an endpoint in `Assign` and the capability, it adds `McpServer::Http { name: "ext", url, headers: [] }` to `session/new` and `session/load`. Without the capability, it emits `harness.mcp_http_unsupported` once and continues. No header, token or credential is passed: identity is the pod IP. The harness already has the CA variables and reaches the gateway host directly (same host and port as `/inference`). This holds for every ACP harness the runtime starts, including `df-native` (`full-harness-mode`) once it exists.

### D10: UI

- Settings area: "MCP servers" page next to "Model providers": list, form (name, URL, credential reference select, scheme with the same preview as model providers), "Fetch tools" with the tool table (select box, name, title, "may write" mark with the bypass warning, new/changed/removed badges, expandable description and schema), "Save selection" (posts the fetch id). Descriptions are rendered as plain text, never as Markdown or HTML (they are untrusted).
- Project settings: tab "MCP servers" with one checkbox per server and the number of pinned tools.
- Run view: "Tool calls (MCP)" table.

The screens reuse the model provider form and the egress tab patterns; a PenPot round is not needed for this slice unless the reviewer asks for it (Q11).

### D11: Who may do it

Board members, through `require_board_member`, like model providers and credentials. When `iter4-permissions` adds roles: registering servers and pinning tools map to `credential:manage` / `agent:configure` (the `admin` role), enabling per project to the project's configuration permission. Only the route guards change.

## Security

- **Untrusted descriptions (tool poisoning).** The model sees only pinned definitions a board member has read. A server that changes a description after review changes nothing until a human refetches. Changed tools come back unselected, so accepting a changed description takes an explicit selection, not a single "Save". A save applies only the fetch the board member saw (fetch id). Only five known fields are served. The UI shows descriptions as plain text, in full, so hidden instructions are visible. Not covered: a malicious description that the reviewer misses.
- **Untrusted results (prompt injection).** Results pass unchanged; the gateway cannot tell data from instructions. Limits: the size cap, no sampling, no elicitation, no roots (an upstream cannot make the model act or ask the human). Consequence: an injected result from server A can make the agent call any other enabled tool, of A or of server B.
- **Write tools bypass the factory's controls.** A write tool acts directly on the upstream system. For a forge server (forgejo-mcp, the motivating example) that means commits, merges, file changes and issues **without** the Git proxy's push checks, secret rules, quarantine and task-branch rules, and without the `create-issue` approval of `chat-issue-tools`. In chats, too, nothing asks a human. Any tool argument also leaves the cluster, read-only tools included (a search query can carry data).
  **Decision for this slice:** nothing is selected by default; tools without `readOnlyHint: true` are marked "may write" with this warning; annotations never select anything. The docs set the rule: do not select write tools of servers that can change the project's forge, repositories or other systems where the factory has its own controls, until the `tool-access` approval or the "ask" permission exists (`iter4-mcp-catalog`, `iter4-permissions`). Write tools are for systems without such controls, where the operator accepts unattended writes. Do not enable a server that reads private data together with one that writes to a public place in the same project.
- **Credentials.** In the control plane and in gateway memory only. The gateway builds upstream requests itself and forwards no sandbox header, so a harness cannot override or read the credential. Nothing from the upstream's response headers goes back to the harness.
- **One credential per server per instance.** The upstream sees all projects' calls under one identity. Any project that enables a server can reach everything that credential can reach (for example all repositories a forgejo token can read), and the upstream's own audit log cannot tell projects apart. One run can also use up the credential's rate limit or quota for all projects; there is no per-run rate limit in this slice. Operators scope the token to what every enabling project may see. Per-project credentials and per-run rate limits: backlog.
- **Any process in the sandbox** can call `/mcp`, as with `/inference`: the trust boundary is the sandbox, not the harness.
- **SSRF.** D6: any resolved address blocked → refused; no redirects; the fetch uses the same client.
- **Availability of other traffic.** Pinned definitions are capped and the snapshot decode limit is raised (D7), so a large selection cannot stop the proxy, Git or inference.
- **Limits.** 1 MiB request, 4 MiB result, 10 s connect, 120 s call, 8 concurrent calls per run and gateway pod; fetch at most 4 MiB, 1,000 tools, 30 s.
- **Permission prompts.** The runtime auto-approves ACP permission requests, so Claude Code runs `ext` tools without asking. That is the existing behavior for all tools.

## Risks / Trade-offs

- [The Claude ACP adapter or OpenCode does not advertise `mcpCapabilities.http`, or ignores HTTP servers] → spikes 1.1 and 1.2 before any runtime work; fallback for OpenCode: a `mcp.ext` remote entry in `OPENCODE_CONFIG_CONTENT`.
- [The harness's MCP client ignores `NO_PROXY` and goes through the CONNECT proxy] → spike 1.1 and the manual check 9.2 look at the egress log; the gateway host is already reached directly for `/inference`.
- [Upstream servers use protocol versions or SSE behaviors the thin client does not handle] → spike 1.3 against Context7 and the fake MCP server; the fake server in tests covers JSON and SSE answers.
- [Pinned schemas go stale; calls fail after an upstream change] → the call returns the upstream's error to the agent; the board member refetches. Automatic detection comes with `iter4-mcp-catalog`.
- [Large pinned sets fill the model context] → humans select tools, at most 100 per server; `search_tools` / `load_server` in the backlog.
- [A repository's `.mcp.json` or `opencode.json` defines servers the harness connects to directly] → those connections go through the egress proxy and need allowlist entries; decision 8 and `iter4-mcp-catalog` handle `.mcp.json`. Spikes 1.1/1.2 check that a repository server named `ext` cannot replace the gateway's (scenarios in the `acp-harness` and `opencode-harness` deltas).
- [Gateway on the path of every MCP call] → same as inference (ADR-0050); a gateway outage already stops agents.
- [Slice size: 5 tables, 3 screens, endpoint, client, reporter, runtime] → accepted; the run view section is the only optional part and can move to a follow-up if the change runs long (records are stored either way).

## Migration Plan

Additive: new tables, new proto fields, new endpoint, a higher decode limit. A run without enabled servers behaves as before. Rollback: disable all servers (no `Assign.mcp_endpoint`, empty tool lists); the migration has a down path that drops the new tables. Release notes name the new page and the docs.

## Decisions to confirm (open questions with recommendation)

| # | Question | Recommendation |
|---|---|---|
| Q1 | Ship this slice before `iter4-mcp-catalog`, reversing the iteration 4 order? | Yes (D1). `iter4-mcp-catalog` becomes the follow-up that adds discovery, flags, filters, persona grants, `tool-access` and `.mcp.json`. |
| Q2 | Enable servers per project or per persona, and opt-in or opt-out? | Per project, opt-in (D1, amends ADR-0032). A server is an external system with its own data; which projects may reach it is the blast-radius decision, and it matches the egress rules (project scope). Persona grants later intersect with it. |
| Q3 | Default for write tools? | Nothing preselected, "may write" mark with the bypass warning, and the documented rule: no write tools of servers that reach systems with factory controls (forge) until `tool-access` / "ask" exists. No technical block, because annotations are untrusted and the human selection is the gate. |
| Q4 | Store call arguments or results? | No. Size and outcome only; the transcript has the payloads (truncated at 64 KiB). |
| Q5 | Who registers servers? | Board members now; `admin` (`credential:manage`) after `iter4-permissions`. |
| Q6 | One credential per server per instance? | Yes, with the consequences documented (shared reach, shared quota). Per-project credentials and per-run rate limits to the backlog. |
| Q7 | Require `http-host` allowlist entries for upstream hosts? | No. Registration is the allow decision (as for model providers). |
| Q8 | Private-destination check for MCP upstreams, although inference and Git skip it? | Yes for MCP (D6). Do not change inference or Git in this change: in-cluster model routers are a real case there and paths are fixed. |
| Q9 | Limits fixed or configurable? | Fixed constants (as for inference). Configuration when a server needs more. |
| Q10 | Harness-visible server name `ext` and separator `__`? | Yes; short because of the 64-character tool name limit. |
| Q11 | PenPot designs for the three screens? | Not required; they follow the model provider form and the egress tab. Ask if the reviewer wants them. |

## Open Questions

- Context7's credential header (`CONTEXT7_API_KEY` or `Authorization: Bearer`) for the docs example: spike 1.4, no change to the design.
