# Sandbox Egress — Deep Dive

> How sandboxes reach anything outside themselves: the gateway Deployment, the allowlist model, generated network policies, how allowlist changes reach running sandboxes, and the live egress log. Decision record: [ADR-0029](adr/0029-egress-gateway-and-generated-network-policies.md). Status: **design draft**.

## 1. Goals

- **Portable:** works with any CNI that enforces upstream `NetworkPolicy`. Cilium is supported, not required.
- **Hostname allowlists without DNS in the sandbox:** sandboxes resolve nothing themselves, which also closes DNS as an exfiltration channel.
- **Changes apply to running sandboxes** — no policy that silently stays stale.
- **Visibility:** a live log of every outbound connection attempt, so missing domains are easy to spot and allow.
- **Attribution:** every connection is tied to a run, task, persona and project.

## 2. Traffic classes

| Traffic | Path | Enforced by |
|---|---|---|
| Agent channel (runtime ↔ control plane) | Direct to the control plane's agent port, mTLS | Network policy |
| HTTP(S) to external hosts (model provider, package registries, forge in v1) | Egress proxy in the gateway (`CONNECT` / plain HTTP forwarding) | Proxy allowlist + network policy (sandbox may only reach the gateway) |
| In-cluster services, any protocol (e.g. a NativeLink scheduler for Bazel RBE) | Direct, as **cluster targets** | Network policy |
| External non-HTTP endpoints | Direct, as **CIDR targets** (escape hatch) | Network policy |
| DNS | Denied by default | Network policy |

## 3. Gateway Deployment

- A separate Deployment (`dark-factory gateway`, same binary as the control plane in a different mode), at least two replicas with a PodDisruptionBudget, scaled independently.
- Hosts the **egress proxy** now; the **Git smart-HTTP proxy** (F1), the **inference endpoint** (F5) and the **MCP endpoint** for third-party MCP servers (ADR-0032) later. These are the bandwidth-heavy data paths, so they stay out of the control plane process.
- Every data path uses its native protocol (HTTP `CONNECT`, Git smart HTTP, OpenAI-compatible HTTP, MCP Streamable HTTP), not the agent protocol. The agent protocol (ADR-0007) remains the control channel between runtime and control plane only.
- The gateway's link to the control plane is a separate internal gRPC API, `darkfactory.gateway.v1`: `PolicyWatch` (§6), `Tail` (§8) and usage/call reports (§9, §10). It gets its own contract when the gateway is built.
- Credentials for data paths (provider keys, forge tokens, upstream MCP credentials) are pushed by the control plane over that API and kept in memory only; the gateway never reads Kubernetes Secrets.
- **No database credentials.** The gateway receives everything it needs from the control plane (§6); it is the component sandbox traffic touches directly, so it holds as little as possible.
- Authenticates to the control plane with mTLS (cert-manager `Certificate`).
- Fail-closed: until a gateway pod has received its first policy snapshot, it denies everything.
- The per-connection allowlist decision ("may this source pod reach host:port?") is an internal interface of the gateway. That keeps the option open to replace only the egress proxy with Envoy later, using the decision as its `ext_authz` service and its access logs as the live log's source (ADR-0029, alternatives).
- Its own network policy: ingress from the sandbox namespace (proxy port) and from the control plane pods (internal API port, for `Tail`); egress to the control plane (`PolicyWatch`) and to the internet on the ports allowlists can use.

## 4. Allowlist model

Entries are stored in the database (ADR-0020) and scoped to **instance**, **project**, **persona** or a single **task**. Harness defaults (e.g. the Claude API host for Claude Code) are instance-level entries attached to a harness. A run's effective allowlist is the union of all scopes that apply to it.

| Kind | Fields | Rendered into |
|---|---|---|
| `http-host` | hostname or `*.suffix`, port (default 443; 80 allowed explicitly) | Gateway allowlist |
| `cluster-service` | namespace, service, ports | Network policy + `hostAliases` + `NO_PROXY` |
| `cidr` | CIDR, ports, protocol | Network policy (`ipBlock` / `toCIDR`) |
| `fqdn` (Cilium mode only) | DNS name or pattern, ports | `toFQDNs` for tools that can't use a proxy |

Allowlist changes need a board-member (later: a dedicated permission) and are written to the audit log.

## 5. Sandbox wiring and generated policies

### 5.1 Pod spec (set at Sandbox creation)
- `HTTPS_PROXY` / `HTTP_PROXY` (and lower-case variants) pointing at the gateway Service's **ClusterIP**, so no DNS is needed to find it. The control plane endpoint is passed the same way.
- `NO_PROXY` lists all `cluster-service` targets, so clients connect to them directly.
- `hostAliases` map each `cluster-service` target's DNS name (`<svc>.<ns>.svc.<cluster-domain>` and `<svc>.<ns>.svc`) to its ClusterIP, so tools resolve them without DNS.
- Headless services have no ClusterIP; a target can opt in to `allowDns`, which allows the cluster DNS for that run (plain `NetworkPolicy` can't restrict DNS by name).

### 5.2 Kubernetes mode (`NetworkPolicy`, default)
One policy per run, selecting the run's pod by label, owned by the Sandbox (deleted with it):
- No ingress.
- Egress to the control plane pods (agent port) and the gateway pods (proxy port).
- Per `cluster-service` target: `namespaceSelector` on `kubernetes.io/metadata.name` plus a `podSelector` copied from the Service's selector, with the Service's **target ports** (policies apply after Service DNAT; named target ports stay named). Services without a selector are rendered from their EndpointSlice addresses as `/32` `ipBlock`s.
- Per `cidr` target: `ipBlock` with ports.
- DNS (kube-dns pods, port 53) only when a target set `allowDns`.

### 5.3 Cilium mode (`CiliumNetworkPolicy`)
The same rules as `toEndpoints` (with the `k8s:io.kubernetes.pod.namespace` label) and `toCIDR`, plus `toFQDNs` for `fqdn` entries (with the DNS rule Cilium needs to observe lookups). Everything else is identical, so switching modes changes rendering only.

### 5.4 Reconciliation
- The control plane leader re-renders the policies of all active sandboxes whenever their inputs change: allowlist entries (via `LISTEN/NOTIFY`), or the Services/EndpointSlices of referenced targets (via informers).
- It also overwrites manual edits (drift), so the effective policy always matches the database. This is the step Paperclip misses.
- Policy changes take effect as fast as the CNI applies them (typically seconds); proxy allowlist changes are faster (§6).

## 6. Mid-run allowlist changes: fan-out

Postgres is involved, but only up to the control plane; the gateway never talks to it.

```
UI / API change
   │  transaction: allowlist rows + policy version bump
   ▼
PostgreSQL ── NOTIFY egress_policy ──► every control plane replica (already LISTENing)
                                              │  PolicyWatch gRPC stream (pushed)
                                              ▼
                                   gateway pods (each connected to one replica)
```

- **PolicyWatch** is a server-streaming RPC from the control plane to each gateway pod. On connect the gateway receives a full, versioned snapshot; afterwards incremental updates.
- The snapshot contains the **sandbox registry** (pod IP → run, task, persona, project) and each run's effective `http-host` allowlist. The control plane updates the registry from its pod informer when sandboxes start and stop.
- Versions are monotonic. On a gap or reconnect the gateway requests a full snapshot; a different control plane replica can serve it because the state comes from the database.
- End-to-end latency is commit + notify + push — typically well under a second.
- **Semantics:** the allowlist is checked per new connection. An added host works on the next attempt (the agent may need to retry; it saw a 403 before). A removed host denies new connections, and the gateway closes that run's open tunnels to it.

## 7. Proxy behaviour

- `CONNECT host:port`: allowed if the host matches the run's allowlist. The gateway then reads the TLS ClientHello and requires the SNI to match the `CONNECT` host, so an allowed name can't front for another destination.
- Plain HTTP forwarding (`GET http://…`): allowed only for entries with port 80 explicitly allowed.
- The gateway resolves names itself and refuses destinations in private, link-local and cluster ranges unless a `cidr` entry allows them. This stops an allowlisted name that resolves to an internal address (or DNS rebinding) from reaching cluster services through the proxy.
- `CONNECT` to IP literals is denied unless a `cidr` entry covers it.
- **Source identity:** the source pod IP is looked up in the sandbox registry; unknown sources are denied and logged. Only pods from the sandbox namespace can reach the gateway at all. Registry entries are removed when a Sandbox is deleted, before its IP can plausibly be reused. (Assumes the CNI preserves pod source IPs for in-cluster traffic, which common CNIs do — verify for the chosen one.)
- **Rejected: TLS interception.** The proxy sees only `CONNECT host:port` and the TLS handshake. Interception would need a CA in every toolchain's trust store, redaction rules for credentials in requests and a retention decision for what is recorded; the project does not want that. Path-level rules and request details (URL, headers) are therefore not available, and the request details panel of the live view says so. Credential injection for private registries is solved elsewhere (Git proxy, ADR-0010).

## 8. Live egress log

Comparable to Hubble: ephemeral, live, filterable — built for spotting missing domains.

- **Per gateway pod:** an in-memory ring buffer of flow events (e.g. the last 10,000). Nothing is persisted per event.
- **Event fields:** timestamp, gateway pod, source pod IP, run / task / persona / project, method (`CONNECT` or HTTP method), host, port, SNI, verdict (allowed / denied) with reason (`not-allowlisted`, `sni-mismatch`, `private-destination`, `unknown-source`), matched rule, bytes in/out, duration, HTTP status for plain HTTP.
- **Relay (fan-in per replica):** each gateway pod serves a `Tail(filter)` gRPC stream (buffer first, then live) on its internal API port, mTLS-authenticated. A viewer's SSE connection is served by exactly one control plane replica; that replica discovers all gateway pods through the EndpointSlices of a headless Service, opens `Tail` to each of them and merges the streams. Replicas don't exchange log events, and Postgres isn't involved (the same pattern as Hubble Relay).
  - Streams exist only while a replica has viewers (and for a 5 s grace period after the last one left, so that the event stream following a page load reuses them); several viewers on one replica share one stream per gateway pod and filter locally. At most control-plane-replicas × gateway-pods streams.
  - Gateway pods appearing or disappearing (EndpointSlice watch) open or close streams.
  - Ordering: buffered events are merged by timestamp on connect; live events are interleaved as they arrive, with a short reorder window. A restarted gateway pod's buffer is lost — acceptable for an ephemeral log.
  - Direction differs from policy updates (§6): policies are pushed to every gateway pod over one stream each (any replica will do, since all replicas receive `NOTIFY`); logs are pulled by whichever replica has a viewer, from all gateway pods.
- **UI (network traffic page):** a split view. Left, a **connection graph**: sandboxes (run id, task key) in a column, external `host:port` destinations in another, one edge per pair; allowed edges solid grey, blocked ones dashed red with a red destination border; nodes and edges the filters exclude are dimmed. Right, the **events table** ("Events · newest first", "N of M"): time, sandbox, destination with host icon, verdict pill (*Allowed*, *Blocked*, *Open*) and actions; in a narrow panel the verdict column becomes a coloured row edge and the actions a menu. A draggable divider (pointer, keys, double click, remembered per browser) separates them. Above: search by domain or IP, the segments *All / Blocked / Allowed* with counts, *Advanced* (persona, sandbox, project, task) and *Clear filters*; the header has *Pause*/*Resume* (closes the stream). A **request details** panel opens from a row and shows what the gateway records: method, status, duration, bytes, destination, SNI, source IP, gateway pod, reason with explanation, matched rule with a link to it on the egress rules page. It states "Requests inside TLS tunnels are not inspected." and shows no URL, query or header. Blocked destinations in the graph and the *Blocked* segment replace the earlier denied-hosts summary.
- **Rejected: per-rule usage** ("last matched", "matched N requests in 24 hours"). Events are memory-only and never persisted, so the control plane cannot know; persisting per-rule usage is not wanted.
- **Allow from the live view** *(planned for iteration 2)*: every denied event (a row of the events table, a blocked node of the graph) has an action to allow that host for the task, project or instance (persona scope follows in iteration 4). It writes an allowlist entry (audited) and goes through the fan-out in §6, so the running agent's next attempt succeeds within about a second — no need to stop or restart the run.
- **Requests from agents** *(planned for iteration 2)*: a denied host also creates a `network-access` approval request on the task, and agents can raise one themselves with a reason (ADR-0038). Approving it writes the same allowlist entry as the live view, at task, project or instance scope; the decision is delivered to the agent as a message, and the run continues without restart.
- **Persisted:** only per-run aggregates (host, verdict, connection count, bytes), stored with the run when it ends, plus allowlist changes in the audit log.
- **Later:** a CLI (`dark-factory egress tail`).
- **Blind spot:** connection attempts that bypass the proxy (tools ignoring proxy settings, wrong ports) are dropped by the network policy and never reach the gateway, so they don't appear in this log. In Cilium mode, Hubble shows them.

## 9. Inference endpoint (later)

- An OpenAI-compatible HTTP endpoint in the gateway (per-provider adapters, F5). The built-in loop uses it in full-harness mode; any ACP harness that accepts a base URL can use it too, so its provider key never enters the sandbox either.
- The sandbox is identified by pod IP, exactly as for the egress proxy; the run's allowed models and token budget come with the `PolicyWatch` snapshot.
- The gateway injects the provider credential, streams responses straight back to the sandbox, measures usage per request and reports it to the control plane (transcript `USAGE_SOURCE_GATEWAY`, ADR-0021). Token streams never pass through the control plane.
- Subscription-based harnesses (Claude Code with `CLAUDE_CODE_OAUTH_TOKEN`) keep talking to their provider through the egress proxy.

## 10. MCP endpoint (later)

- One MCP endpoint (Streamable HTTP) aggregating the third-party MCP servers a run may use (ADR-0032). The runtime registers it in the harness configuration next to its own `factory` server.
- Same identification (pod IP) and the same `PolicyWatch` snapshot as the proxy; the snapshot carries the run's tool set per upstream server.
- The gateway injects upstream credentials, filters `tools/list`, rejects calls outside the tool set, and offers `search_tools` / `load_server` when the set is large.
- Tool catalogs and their instance-level filters live in the control plane (discovery, new tools disabled by default); the gateway only receives the result.
- One record per call (run, server, tool, status, duration, size) is reported to the control plane, alongside the egress aggregates.

## 11. Compatibility notes

- Claude Code documents `HTTPS_PROXY` support; verify through the ACP adapter.
- git, curl, cargo, npm, pip and Bazel's downloader honour proxy environment variables.
- Bazel RBE against an in-cluster NativeLink scheduler uses a `cluster-service` target and bypasses the proxy (`NO_PROXY`).
- A harness may contact more hosts than its API (telemetry, update checks). Start from the live log; disable what isn't needed through the harness's own settings.

## 12. Future option: DNS server in the gateway

Like the HTTP proxy, the gateway could serve DNS for sandboxes: the pod spec sets `dnsPolicy: None` and a `dnsConfig` pointing at the gateway's ClusterIP, and the network policy allows DNS only to the gateway.

- **Per-run answers:** queries are attributed by source pod IP like proxy connections; the resolver answers only names on the run's allowlist and cluster targets, and returns `NXDOMAIN` for everything else. That keeps DNS closed as an exfiltration channel while letting tools resolve names normally.
- **Cluster targets without workarounds:** headless Services resolve to their pod IPs, so `hostAliases` and the `allowDns` opt-in (§5.1) become unnecessary.
- **Visibility:** queries go into the live egress log. Tools that ignore the proxy usually still look up the name first, so they would show up as DNS events — narrowing the blind spot in §8.
- **Limit:** resolving a name grants no connectivity. Direct (non-proxied) connections still need a network policy that allows the resolved addresses; dynamic, DNS-driven policies are what Cilium's FQDN rules provide, so this would remain Cilium mode's job.

## 13. Open questions
- Per-run aggregates: store in the run record or a separate table for cross-run reporting.
