# ADR-0065: Health probes on a private HTTP port

- **Status:** Proposed (change `private-health-port`)
- **Date:** 2026-11-27
- **Related:** ADR-0029

## Context

The control plane served `/healthz` and `/readyz` on its UI port, which the ingress publishes, so anyone could make it ping the database. The gateway already served its probes on a separate health port that no Service exposes. Both binaries are gRPC services, and Kubernetes has native gRPC probes (`grpc.health.v1`, stable since v1.27). But the kubelet's gRPC probe connects in plaintext by default; its optional TLS mode (feature gate `GRPCContainerProbeTLS`, alpha since v1.37, off by default) skips certificate verification and sends no client certificate. Every gRPC port of Dark Factory requires a client certificate from the instance CA.

## Decision

The control plane and the gateway serve `/healthz` (liveness, no dependency) and `/readyz` (readiness) over plain HTTP on a health port of their own. No Service, no HTTPRoute and no NetworkPolicy rule names it; the kubelet reaches it as node traffic, which NetworkPolicy always allows. No gRPC health service.

## Consequences

- Probes are unreachable from the ingress, from other pods and from sandboxes; `/readyz` may ping the database on every call.
- One more port in the control plane's values (`controlPlane.ports.health`).
- A CNI that applies policy to host traffic needs a rule for the health port (documented, not rendered).
- External uptime checks can no longer use `/healthz` through the ingress.

## Alternatives considered

- **gRPC health checks** (`tonic-health`, `probe.grpc`): would still need a plaintext gRPC listener, so a private port anyway; adds a dependency and a second tonic server, stores a status instead of checking per call, needs numeric probe ports and `grpcurl` for manual checks. No client uses gRPC health. Rejected.
- **Keep the probes on the UI port and block them at the ingress**: depends on each operator's ingress configuration. Rejected.
- **Probes and `/metrics` on one port**: `/metrics` is already on its own private port, configured by the OpenTelemetry variables and reachable from the scraper namespace; merging would open `/readyz` to that namespace. Rejected for now (open question in the change).
