# ADR-0019: Identifiers

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

Identifiers appear in URLs, branch names, PVC names (63-character limit), refs and the audit log.

## Decision

- UUIDv7 internally.
- Short per-project keys for humans, branches and Kubernetes names, e.g. `DF-42` → branch `factory/task/DF-42`, PVC `task-df-42`.

## Consequences

- Branch and resource names stay readable and short.

## Alternatives considered

- **UUIDs everywhere**: unreadable branch names and long Kubernetes names.
