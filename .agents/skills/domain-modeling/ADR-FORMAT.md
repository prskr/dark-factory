# ADR Format

ADRs live in `docs/adr/` with four-digit sequential numbers: `0046-forge-activity-in-the-transcript.md`. Scan `docs/adr/` for the highest number and add one.

## Template

Copy [`docs/adr/template.md`](../../../docs/adr/template.md):

```md
# ADR-NNNN: Title

- **Status:** Proposed | Accepted | Superseded by ADR-NNNN | Deprecated
- **Date:** YYYY-MM-DD
- **Related:** ADR-NNNN

## Context

What is the situation and which forces are at play?

## Decision

What was decided, stated in active voice.

## Consequences

What becomes easier or harder because of this decision?

## Alternatives considered

Which options were rejected, and why?
```

Keep each section short and technical. Link related ADRs by relative path (`[ADR-0017](0017-transcript-schema-and-versioning.md)`).

## Index

Every new ADR gets a row in the table of [`docs/adr/README.md`](../../../docs/adr/README.md): `| [ADR-NNNN](NNNN-slug.md) | Title | Status |`. When an ADR amends or supersedes another, update the older ADR's status there and in its file ("Accepted (amended by ADR-NNNN)", "Superseded by ADR-NNNN").

A decision is never edited into its opposite: write a new ADR that supersedes it.

## When to offer an ADR

All three of these must be true:

1. **Hard to reverse**: the cost of changing your mind later is meaningful
2. **Surprising without context**: a future reader will look at the code and wonder "why on earth did they do it this way?"
3. **The result of a real trade-off**: there were genuine alternatives and you picked one for specific reasons

If a decision is easy to reverse, skip it: you'll just reverse it. If it's not surprising, nobody will wonder why. If there was no real alternative, there's nothing to record beyond "we did the obvious thing."

### What qualifies

- **Architectural shape.** "Pushes are proxied through the gateway." "The transcript is one ordered stream per task."
- **Integration patterns between components.** "The runtime dials back to the control plane over gRPC with mTLS."
- **Technology choices that carry lock-in.** Database, protocol, deployment target. Not every library: just the ones that would take a quarter to swap out.
- **Boundary and scope decisions.** "Code review stays in the forge." The explicit no-s are as valuable as the yes-s.
- **Deliberate deviations from the obvious path.** Anything where a reasonable reader would assume the opposite.
- **Constraints not visible in the code.** "Kubernetes only." "Single-tenant instances."
- **Rejected alternatives when the rejection is non-obvious.**

Implementation plans for a decision belong in an OpenSpec change (`openspec/changes/<name>/design.md`), not in the ADR.
