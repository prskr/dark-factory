# Proposal

## Why

The control plane serves `/healthz` and `/readyz` on its UI port (`controlPlane.ports.http`, 8080), the port the HTTPRoute and the ingress controller publish. Anyone on the internet can call `/readyz` and make the control plane ping its database. Probes are for the kubelet only; they must not be reachable through the ingress. Found in the MCP proxy review (DF-26, PR #336).

## What Changes

- **BREAKING (probe paths):** The control plane serves `/healthz` and `/readyz` only on a new **health port** (instance config `server.healthPort`, Helm `controlPlane.ports.health`, default `8081`). The UI port answers `404` for both paths.
- The health port is plain HTTP, like the gateway's existing health port (`gateway.healthPort`, Helm `gateway.ports.health`, `8080`, unchanged). Both binaries use the same approach. gRPC health checks (`grpc.health.v1`) were compared and rejected: the kubelet's gRPC probe cannot do mTLS, so it would need its own plaintext listener anyway (design D1, ADR-0065).
- The health port is not in any Service, not in the HTTPRoute, and no NetworkPolicy rule allows it. The kubelet reaches it as node traffic, which Kubernetes NetworkPolicy always allows. The docs name the CNI setting for clusters that filter host traffic (Cilium, design D4).
- Bind errors of the control plane and gateway listeners name the configuration key and port.
- The Helm probes of the control plane target the health port. The readiness probe gets `timeoutSeconds: 3`, longer than the database ping timeout of `/readyz` (2 s); today the kubelet default of 1 s cuts the ping short.
- `/readyz` keeps its behavior: the control plane pings the database with a 2 s timeout on every call, the gateway is ready after its first policy snapshot. No cached result.
- Metrics stay where they are: the Prometheus endpoint has its own port (`telemetry.metrics.port`, 9464), reachable only from `networkPolicy.metricsScraper`. Open question Q1 in the design.
- Other routes without login stay on the UI port: static assets, `/lifecycle-events/public-key`, `/login`, `/auth/callback`, `/logout`, `/webhooks/{plugin}`.
- Docs: Helm README, values comments, crate READMEs of `df-web` and `df-gateway`, published pages (architecture, authentication, telemetry, upgrades) and `docs/authorization.md` describe the health ports.

## Capabilities

### New Capabilities

- `health-probes`: liveness and readiness endpoints of the control plane and the gateway, what they check, and that they are served only on a private health port.

### Modified Capabilities

- `helm-deployment`: the instance network policies leave the health ports closed to every pod; new requirement for the probe wiring (health port values, probes, no Service or HTTPRoute port).

## Impact

- **Code:** `crates/df-core/src/config.rs` (`ServerConfig::health_port`), `crates/df-web/src/lib.rs` (probe routes leave `base_router`, own health router), `crates/dark-factory/src/main.rs` (health listener of `serve`, tests), `crates/df-gateway` (no behavior change, docs only).
- **Helm:** `values.yaml`, `values.schema.json`, `templates/configmap.yaml`, `templates/control-plane-deployment.yaml`, `tests/control_plane_test.yaml`, `README.md`. The new value `controlPlane.ports.health` has a default, so existing values files need no change (Helm merges value maps); the breaking part is that the probe paths left the UI port.
- **Dependencies:** none added (no `tonic-health`).
- **E2E and local dev:** nothing waits on the control plane's `/healthz` outside Kubernetes probes; `helm --wait` in `deploy/e2e/e2e-local.sh` follows the readiness probe and needs no change. `compose.yml` is not affected. `just serve-local` opens the health port on `8081`.
- **Domain model:** unchanged (no glossary term added or changed). Adds ADR-0065 "Health probes on a private HTTP port".
