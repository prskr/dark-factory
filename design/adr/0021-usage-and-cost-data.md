# ADR-0021: Usage and cost data

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

Claude Code runs mainly on subscriptions, so the "what if" cost at API prices is the primary cost view.

## Decision

- Recorded per turn: model id plus input, output, cache-read and cache-write tokens.
- Pricing tables live in the instance configuration, versioned by date.

## Consequences

- "What if" costs for past runs stay reproducible.
- Depends on the Claude ACP adapter reporting usage; fallback: Claude Code's session JSONL.

## Alternatives considered

- **Storing computed costs only**: prices change; raw tokens keep history recomputable.
