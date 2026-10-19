# ADR-0020: Configuration lives in the database

- **Status:** Accepted (amended by [ADR-0039](0039-repository-configuration-activated-by-approval.md): approved repository configuration as an additional project layer, stored in the database)
- **Date:** 2026-10-16
- **Related:** [ADR-0011](0011-authentication-authorization-personas.md)

## Context

Personas, harness configuration, skills, projects and routines must be stored somewhere. Kubernetes custom resources were an option.

## Decision

- The database is authoritative for this configuration; no custom resources.
- Iteration 1: minimal UI forms; credentials are entered as references to existing Kubernetes Secrets (name + key), never as values.
- A declarative import (F4 in `product/context.md`) comes later.

## Consequences

- Agents managing personas (ADR-0011) don't need Kubernetes API access.
- One store instead of two.

## Alternatives considered

- **Custom resources**: two stores, and agent-driven instance management would need write access to the Kubernetes API.
