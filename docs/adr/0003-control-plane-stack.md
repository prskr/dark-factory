# ADR-0003: Control plane stack

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0012](0012-postgresql-and-blob-store.md)

## Context

The control plane is a single binary running as a Kubernetes Deployment, possibly with several replicas. It serves the web UI and integration API, the agent port, provisions sandboxes and hosts plugins. It should need as few stateful dependencies as possible.

## Decision

| Concern | Choice |
|---|---|
| Runtime / HTTP | tokio, axum, utoipa (OpenAPI for the integration API) |
| UI streaming | Server-sent events from axum |
| Agent channel | tonic gRPC bidirectional stream on a separate port, mTLS (ADR-0006) |
| Kubernetes | kube-rs, `kopium`-generated agent-sandbox types, Lease-based leader election |
| Storage | PostgreSQL via sqlx, plus a blob store abstraction (ADR-0012) |
| Full-text search | Postgres `tsvector` + `pg_trgm`; pgvector later for agent memory |
| Task queue / routines | Postgres `FOR UPDATE SKIP LOCKED` + `croner` |
| Multi-replica fan-out | Persist each event first, then broadcast via Postgres `LISTEN/NOTIFY`, so any replica can serve any UI stream |
| MCP | Factory tool calls arrive over the agent stream (ADR-0027); `rmcp` client to discover upstream MCP servers' tool catalogs; upstream MCP traffic itself goes through the gateway (ADR-0032) |
| WASI | wasmtime, wasmtime-wasi, wasmtime-wasi-http |
| Secrets | Kubernetes Secrets, referenced from the database |
| Auth | OIDC for humans, mTLS for agents (ADR-0011) |
| Telemetry | tracing + OpenTelemetry |

## Consequences

- PostgreSQL is the only stateful dependency: it provides storage, search, the job queue and cross-replica notifications.
- Secrets fit GitOps / External Secrets because only references are stored.

## Alternatives considered

- **A message broker (e.g. NATS) for queues and fan-out**: an extra stateful component; Postgres covers both at this scale.
- **A dedicated search engine**: Postgres full-text search is sufficient for a handful of users.
