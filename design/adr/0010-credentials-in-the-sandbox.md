# ADR-0010: Credentials in the sandbox

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0006](0006-agent-dial-back-grpc-mtls.md), [ADR-0009](0009-agent-managed-workspaces-git-durable.md)

## Context

Sandboxes run untrusted model-driven code. Any credential inside a sandbox is available to the harness and to anything it executes.

## Decision

- Target state: the only credentials in a sandbox are its mTLS identity (per pod, short-lived, bound to pod UID and IP) and third-party harness authentication (e.g. a Claude subscription token).
- No Kubernetes ServiceAccount token, no inference provider keys, no third-party MCP credentials — they stay in the gateway, which injects them into inference and upstream MCP traffic (ADR-0029, ADR-0032).
- v1 exception: a short-lived, repo-scoped forge token served on demand by the runtime's Git credential helper, until the Git proxy (F1 in `product/context.md`) exists.

## Consequences

- Removing tokens doesn't remove authority: the sandbox still acts through the control plane, so per-task authorization there is the real control.
- Project build/test secrets (private registries, test databases) are out of scope for now.

## Alternatives considered

- **Long-lived tokens in the pod environment**: exposed to the harness for the sandbox's whole lifetime.
