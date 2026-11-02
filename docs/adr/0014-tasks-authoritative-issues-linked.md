# ADR-0014: Tasks are authoritative; issues are linked

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0015](0015-webhook-filtering-and-routing.md)

## Context

Work arrives from an issue tracker (Forgejo first) but is executed and tracked in Dark Factory. One side has to be the source of truth.

## Decision

- The Dark Factory database is the source of truth for tasks.
- An issue can have many tasks; a task links to at most one issue. Tasks without issues are allowed.
- Task progress is mirrored to the linked issue in whatever form fits the tracker (comments, labels, status) via the issue-tracker plugin.
- Agents can comment on issues through a control-plane tool to give and preserve context.

## Consequences

- The issue-tracker plugin interface is mostly read plus comments, not a two-way sync.

## Alternatives considered

- **Issue tracker as source of truth**: would require two-way synchronization and tracker-specific task state.
