# ADR-0016: Runs and triggers

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0015](0015-webhook-filtering-and-routing.md), [ADR-0026](0026-chats-dependencies-project-context.md)

## Context

Tasks are interrupted — questions to a human or another agent, limits, failures — and must resume with their history. Several sources can wake a task.

## Decision

- Hierarchy: issue 1 → n tasks, task 1 → n runs; runs of a task are strictly sequential.
- A run ends when the agent finishes, asks a question or requests approval, hits a limit (tokens, time, provider rate limit), or fails. A provider rate limit is a distinct, resumable end reason.
- History belongs to the task: all runs share one transcript and context. Continuity via the harness's session state on the PVC where possible, otherwise a summary.
- Humans can queue messages at any time; delivery is turn-based, like ACP.
- Blocking questions keep the sandbox alive for a short window; later answers start a new run.
- Triggers: assignment (human or superior agent), messages, routines, completion of a task this one depends on (ADR-0026), routed webhook events (ADR-0015).
- Delegation: a parent doesn't block on children; a child finishing queues a message on the parent.
- Event-driven plus routines only; no Paperclip-style heartbeats unless something turns out to be missing.

## Consequences

- Idle sandboxes don't hold resources while waiting for humans.

## Alternatives considered

- **Heartbeats** (periodic wake-ups): deferred until a need shows up.
