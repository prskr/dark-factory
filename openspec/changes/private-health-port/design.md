# Design

## Context

See proposal.md, "Why". Current state, checked on 2026-11-27:

| | Control plane | Gateway |
|---|---|---|
| Probe routes | `df_web::base_routes` (`crates/df-web/src/lib.rs`), merged into every UI router | `df_gateway::health::router` (`crates/df-gateway/src/health.rs`) |
| Port | UI port `server.httpPort` / `controlPlane.ports.http` (8080) | `gateway.healthPort` / `gateway.ports.health` (8080), own listener |
| In a Service | yes (`control-plane-service.yaml`, port `http`); HTTPRoute backend | no (`gateway-service.yaml` has proxy, git, internal-grpc, metrics) |
| NetworkPolicy | UI port open to the ingress controller, or to every source with `ingressController.allowAll` | no rule for the health port |
| `/readyz` | database ping, `READY_TIMEOUT` 2 s, no cache | first policy snapshot received (`watch` borrow) |
| Probe timeout | kubelet default `timeoutSeconds: 1` (not set in the template) | default 1 s, check is a memory read |

So the gateway already has what this change wants; only the control plane moves. Metrics: `df-telemetry` serves `GET /metrics` on its own listener (`OTEL_EXPORTER_PROMETHEUS_PORT`, default 9464, `telemetry.metrics.port`), started before the command runs. It is in both Services as port `metrics`, and the NetworkPolicy opens it only to `networkPolicy.metricsScraper`. Nothing else serves for scraping.

The kubelet's gRPC probe, checked against the Kubernetes docs (kubernetes/website, `configure-liveness-readiness-startup-probes.md`, main branch, 2026-11-27): stable since v1.27; "The probes connect over plaintext by default"; a `mode: TLS` behind the feature gate `GRPCContainerProbeTLS` connects with `InsecureSkipVerify` and "Certificate verification is not supported"; no client certificates; "gRPC probes do not support named ports". The feature gate list (kubernetes.io, v1.37 docs) shows `GRPCContainerProbeTLS` as alpha since 1.37, default `false`.

NetworkPolicy, checked against the Kubernetes docs (`network-policies.md`, same date): "traffic to and from the node where a Pod is running is always allowed", and an isolated pod accepts "those from the pod's node and those allowed by the `ingress` list". The gateway's health port works today without any rule, in the e2e kind cluster as well.

## Goals / Non-Goals

**Goals:**

- `/healthz` and `/readyz` of both binaries are reachable only by the kubelet (and other node-local traffic), never through the ingress, the Services or from sandboxes.
- One approach for both binaries.

**Non-Goals:**

- Moving other routes without login off the UI port. Not wanted: they must stay public (rejected).
- Moving `/metrics` onto the health port (Q1, recommendation: keep it separate).
- A startup probe or a deeper readiness check (forge plugins, cluster API). Not wanted now: readiness gates traffic to the UI and agent port, and only the database blocks both (rejected).
- gRPC health service for clients of the internal gRPC ports (rejected, D1).

## Decisions

### D1. Private HTTP health port for both binaries, not gRPC health

Compared:

| | Private HTTP port | gRPC `grpc.health.v1` |
|---|---|---|
| Listener | one plain HTTP listener per binary; the gateway has it already | needs a plaintext gRPC listener: the kubelet cannot present a client certificate, and our gRPC ports (agent 9443, internal 9444) require one from the instance CA. Even with `mode: TLS` it would fail the mTLS handshake. So it is a private port as well |
| Dependency | none (axum is in both crates) | `tonic-health` and a second tonic server per binary |
| Liveness / readiness | two paths: `/healthz`, `/readyz` | service `""` for liveness (`SERVING` once started), a named service, for example `dark_factory.Readiness`, for readiness, selected with `probe.grpc.service` |
| Per-call check | the handler pings the database on each call | `tonic-health`'s `HealthReporter` stores a status that someone sets; a database ping per call needs an own `Health` implementation, otherwise a background task and a cached, possibly stale status |
| Probe spec | `httpGet` with named port | `grpc.port` must be a number (no named ports) |
| Manual checks | `curl` | `grpcurl` or `grpc_health_probe`, not in the images |
| Who else uses it | nobody | nobody: the gateway's `PolicyWatch` and the control plane's `Tail` already reconnect on failure; no client-side load balancing reads health |

gRPC adds a dependency and a listener and gains nothing, because the probe would still need a private plaintext port. Decision: a private plain HTTP health port in both binaries, same paths as today. Recorded as ADR-0065.

### D2. Ports and names

- Control plane: new `server.healthPort` (`ServerConfig::health_port`, default `8081`), Helm `controlPlane.ports.health: 8081`, container port name `health`. `8081` because the UI keeps `8080`.
- Gateway: unchanged (`gateway.healthPort`, `gateway.ports.health: 8080`, port name `health`). Moving it to `8081` for symmetry would only churn the values (rejected).
- Same port name `health` and same paths in both Deployments, so probes, docs and tests read the same.
- A health port equal to another port of the same binary fails at bind with "address in use"; no extra config validation.

### D3. Where the code lives

- `df-web`: `base_router` loses `/healthz` and `/readyz` (it keeps assets and the lifecycle key). A new `df_web::health_router(pool)` returns only the two probe routes with the existing `readyz` handler and `READY_TIMEOUT`. It stays in `df-web` because `readyz` and its test helpers are there; a move to its own crate is not worth it for two routes.
- `dark-factory serve`: binds `server.healthPort` next to the other listeners in `run_serve_command`, passes it into `serve`, and spawns `serve_health(listener, health_router)` under `until_stopped` like `serve_ui`, after the startup checks. The `request_span` layer stays on, so the `telemetry` spec ("spans for … health endpoints") still holds.
- Gateway: no code change.

### D4. Network policy and CNIs

No allow rule for the health ports. Kubernetes NetworkPolicy always allows traffic from the pod's own node, which is where the kubelet probes from; an `ipBlock` for node CIDRs would also open the port to every pod with `hostNetwork` on any node and needs a cluster-specific value. Pods of the release namespace and the sandboxes cannot reach the port because no rule names it.

Cilium: by default it allows traffic from the local host to its endpoints, so probes work; only a cluster that configured Cilium to apply policy to host traffic needs a `CiliumNetworkPolicy` with `fromEntities: [host]` (or `remote-node`) for the health port. The Cilium option name and default (`allow-localhost`, default `auto`) are (unverified); task 1.1 verifies them before the docs name them. The chart renders no Cilium objects; the Helm README and the security page say what to add.

With `networkPolicy.enabled: false` every pod of the cluster can reach the health port by pod IP. That is the same as today for every port; the docs say so.

### D5. What `/readyz` checks and how often

- Control plane: database ping on every call, 2 s timeout (`READY_TIMEOUT`), no cache. The readiness probe runs every 5 s per pod: 0.2 pings per second per replica, cheap. A cache only helps against callers we no longer have (the internet).
- Helm: readiness `timeoutSeconds: 3` (> 2 s), so the kubelet gets the `503` instead of timing out on its default 1 s. Liveness keeps the default timeout (no dependency).
- Gateway: unchanged (first policy snapshot).

### D6. Tests and e2e

- Unit tests in `crates/dark-factory/src/main.rs` (`ui_port_requires_login_when_auth_is_configured`, `ui_is_disabled_without_auth_but_health_stays_up`, and the `serve` tests `serve_runs_the_agent_port_next_to_the_ui_port` and `serve_starts_with_the_charts_rbac` that call `/healthz` on the UI listener) move to the health listener and gain assertions that the UI port answers `404` for `/healthz` and `/readyz`, with and without `auth`.
- `crates/df-web/src/auth/tests.rs` (`health_and_assets_need_no_login`) keeps the assets check on the UI router, asserts `404` there for the probe paths, and tests the probes on the health router.
- Helm unit tests (`deploy/helm/dark-factory/tests/control_plane_test.yaml`): probe port `health`, `timeoutSeconds: 3`, Service without `health`, NetworkPolicy without the health port (also with `allowAll`), schema rejects a missing `controlPlane.ports.health`.
- E2E: `deploy/e2e/e2e-local.sh` waits through `helm --wait` (readiness probes) and uses the UI port only for the UI and webhooks; no change. `compose.yml` and `dev/config.yaml` set no ports; no change. The e2e suite gets no new test: the Rust and Helm tests cover the behavior, and the e2e cluster proves the probes work when the release becomes ready.

## Risks / Trade-offs

- [A CNI or cluster policy filters node traffic, so probes fail and pods never become ready] → Visible at once on install (pods not ready, probe events). Docs name the Cilium setting (D4). Same risk exists today for the gateway, without reports.
- [Operators or monitoring call `https://<publicUrl>/healthz` today for uptime checks] → Pre-MVP, no installations. The upgrade notes say to use a check on the UI (for example `/login` answers `303`, or `/assets/…`) or the metrics.
- [Someone sets `controlPlane.ports.health` equal to `controlPlane.ports.http`] → Bind fails at startup with "address in use", the pod crash-loops with a clear log line.

## Migration Plan

1. Release with the new value `controlPlane.ports.health` (default set; schema requires it). `helm upgrade` with default values works without changes.
2. Values files that list `controlPlane.ports` explicitly add `health`. Upgrade notes (`docs/content/operations/upgrades.md`) say so and that `/healthz` and `/readyz` are gone from the UI port.
3. Rollback: the previous chart and image restore the probes on the UI port; no data involved.

## Open Questions

- **Q1. Metrics on the health port?** The task asked to put anything served for scraping on the same private port. Recommendation: keep `/metrics` on its own port. It is already private (NetworkPolicy: scraper only), it follows the OpenTelemetry variables `OTEL_EXPORTER_PROMETHEUS_HOST`/`_PORT` that operators know, `df-telemetry` starts it before the command and config are loaded, and its audience (the scraper namespace) differs from the health port's (node only). Merging would open `/readyz` to the scraper namespace and tie `df-telemetry` to both binaries' listeners.
- **Q2. Gateway health port `8080` or `8081`?** Recommendation: keep `8080` (D2). Symmetry with the control plane would change a value for no gain.
- **Q3. Optional NetworkPolicy rule for probe sources?** For example `networkPolicy.probeSources` (a list of `ipBlock`s) for CNIs that filter node traffic. Recommendation: no value now; document the CNI setting instead (D4). Add it when a user needs it.
