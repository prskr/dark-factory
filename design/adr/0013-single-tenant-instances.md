# ADR-0013: Single-tenant instances

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

Paperclip is multi-tenant, likely rooted in a SaaS ambition. Dark Factory isn't meant to be sold (for now).

## Decision

- One Dark Factory instance (control plane Deployment + database, typically its own namespace) per tenant.
- No tenant id columns, no tenant scoping in queries or authorization.

## Consequences

- Isolation between tenants comes from Kubernetes (namespaces, network policies, separate databases), not application logic.
- Rust's small footprint makes several instances cheap.

## Alternatives considered

- **Multi-tenancy in the application**: complexity everywhere to save a few megabytes of RAM per tenant.
