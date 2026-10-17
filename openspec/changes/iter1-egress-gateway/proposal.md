# Proposal

## Why

Sandboxes have no direct internet access; Claude Code, package registries and Forgejo (v1) must be reached through an allowlisting proxy, and a live log is needed to find out which hosts the harness actually contacts. ADR-0029 puts this in a separate gateway Deployment; iteration 1 includes a minimal version (`product/iterations/iteration-1.md` §3, interview answer 9).

## What Changes

- New internal contract `contract/proto/darkfactory/gateway/v1/gateway.proto`: `PolicyWatch` (control plane → gateway, snapshot + updates) and `Tail` (gateway → control plane, live flow events).
- `dark-factory gateway` mode in `df-gateway`: HTTP `CONNECT` proxy (and plain HTTP for explicit port-80 entries) with per-run `http-host` allowlist, source identification by pod IP, SNI check, own DNS resolution with private/link-local/cluster destination blocking, fail-closed until the first snapshot, in-memory ring buffer of flow events, `Tail` server, mTLS on the internal port.
- Control-plane side: `PolicyWatch` server on the internal port (snapshots built from active runs' pod IPs and instance/project allowlist entries, pushed on `egress_policy`/`run_changed` notifications), `Tail` relay merging all gateway pods into one SSE stream.
- Desktop-only UI pages: allowlist editor (instance and project scope; `http-host` and `cluster-service` entries, audited) and live egress log with filters and a denied-hosts summary.

## Capabilities

### New Capabilities
- `egress-proxy`: what the proxy lets through and how it decides.
- `egress-policy-distribution`: how allowlists and the sandbox registry reach gateway pods.
- `egress-allowlist`: managing allowlist entries.
- `egress-log`: the live log of connection attempts.

### Modified Capabilities
None.

## Impact

- New contract file, new crate `df-gateway` (gateway mode + control-plane relay/server), `dark-factory gateway` subcommand, two page modules in `df-web` (reusing its layout and auth from `iter1-control-plane-web`).
- Reads `runs.pod_ip` (written by `iter1-run-orchestration`) and `egress_allowlist` (foundation schema); edits to `cluster-service` entries trigger policy re-rendering in `iter1-sandbox-provisioning`.
- Deferred per iteration-1 §3: persona/task scopes, CIDR targets, Cilium mode, one-click allowlisting, per-run persisted aggregates.
- New dependencies: hyper, hyper-util, tls-parser, hickory-resolver.
