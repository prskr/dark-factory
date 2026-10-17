# Design

## Context

`design/egress.md`, ADR-0029. The gateway is the same binary as the control plane (`dark-factory gateway`), ≥ 2 replicas, no database credentials, no Kubernetes API token. Ports: `iter1-foundation` design D7. Pod wiring (proxy env, network policy to gateway pods) is done by `iter1-sandbox-provisioning`; chart objects by `iter1-build-and-deploy`.

## Goals / Non-Goals

**Goals:**
- Proxy decision logic is a pure function `decide(snapshot, source_ip, request) -> Verdict`, unit-tested exhaustively.
- The contract `darkfactory.gateway.v1` is written first so both sides can be built in parallel.

**Non-Goals:**
- TLS interception, Git proxy, inference and MCP endpoints, DNS server, Envoy, CLI tail, persisted aggregates, one-click allowlisting.

## Decisions

### D1 — Contract sketch
```proto
service GatewayPolicy { rpc PolicyWatch(PolicyWatchRequest) returns (stream PolicyUpdate); }  // served by control plane
service GatewayLog    { rpc Tail(TailRequest) returns (stream FlowEvent); }                  // served by gateway
message PolicyUpdate { uint64 version = 1; Snapshot snapshot = 2; }
message Snapshot { repeated SandboxEntry sandboxes = 1; }
message SandboxEntry { string pod_ip = 1; string run_id = 2; string task_key = 3;
                       string project_key = 4; string persona_id = 5; repeated HostRule hosts = 6; }
message HostRule { string pattern = 1; uint32 port = 2; string scope = 3; string entry_id = 4; }
```
*ponytail:* every update is a full snapshot (small in iteration 1: a handful of runs). Incremental deltas are an additive change when snapshot size matters. Versions come from a Postgres sequence bumped on each rebuild.

### D2 — Control-plane side
`df_gateway::control` builds the snapshot with one query (active runs with `pod_ip` × instance + project `http-host` entries). Each `PolicyWatch` stream subscribes to `egress_policy` and `run_changed` notifications, debounces 100 ms, rebuilds, sends. Served on port 9444 with a rustls verifier requiring the instance CA; the gateway's certificate SAN is checked against the configured gateway identity.

### D3 — Proxy
hyper server on 3128 handling `CONNECT` (upgrade, then peek the ClientHello with `tls-parser` before connecting upstream) and absolute-form HTTP requests. Name resolution with `hickory-resolver`; all resolved addresses are checked against blocked ranges (RFC 1918, loopback, link-local, CGNAT 100.64/10, ULA, the configured pod/service CIDRs) before connecting to the first allowed one. Open tunnels are tracked per (run, host); on a snapshot that removes the host, they are aborted.

### D4 — Ring buffer and Tail
`VecDeque` behind a mutex plus a `broadcast` channel for live events; `Tail` replays matching buffered events, then follows the channel (lagging receivers drop events and get a gap marker). Default capacity 10,000.

### D5 — Gateway pod discovery for the relay
The relay resolves the gateway headless Service's DNS name every 5 s and opens/closes `Tail` streams per pod IP. *ponytail:* DNS instead of an EndpointSlice watch — no Kubernetes client in this path; switch if readiness lag matters. One shared set of streams per replica, fanned out to viewers through a `broadcast` channel and filtered per viewer.

### D6 — UI
Two route modules in `df-web` using its `layout` and auth: `/egress/allowlist` (instance and per-project entries, forms with validation; `cluster-service` validation via `df-k8s` Service lookup) and `/egress/log` (SSE, filters as query parameters, denied-hosts summary aggregated in the relay over the current stream).

## Risks / Trade-offs

- [Pod source IP not preserved with Kata/Cilium] → verified by `iter1-sandbox-provisioning` spike 1.4; without it the proxy can't attribute traffic and the design needs revisiting.
- [Full-snapshot updates grow with many runs] → acceptable at homelab scale; deltas are additive in v1.
- [Claude contacts undocumented hosts] → the denied-hosts summary is exactly the tool to find them during the demo setup.
