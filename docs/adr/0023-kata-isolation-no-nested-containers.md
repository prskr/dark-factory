# ADR-0023: Kata isolation, no nested containers

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0022](0022-project-images-runtime-volume-no-warm-pools.md)

## Context

Agent sandboxes already run with Kata Containers in the homelab. Agents need to run integration tests that use containers.

## Decision

- Sandboxes run with a configurable runtime class; Kata in the homelab.
- No nested containers or container daemons inside sandboxes. Container-based tests run through Bazel remote build execution, whose workers run the container daemons; RBE endpoints are allowed per project in the egress policy.

## Consequences

- Sandbox templates stay simple (no rootless BuildKit/Podman).

## Alternatives considered

- **Container daemons in sandboxes**: complex templates and a larger attack surface.
