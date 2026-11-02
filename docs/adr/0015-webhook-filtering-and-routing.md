# ADR-0015: Inbound events: plugins normalize, the control plane filters and routes

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0014](0014-tasks-authoritative-issues-linked.md), [ADR-0016](0016-runs-and-triggers.md), [ADR-0025](0025-pr-and-branch-handling.md)

## Context

Forges send webhooks for CI results, PR reviews and issue comments. Not every event should wake an agent — e.g. Forgejo creates several review comments before a review is submitted.

## Decision

- The control plane owns the webhook endpoint. The plugin parses the request (`handle-webhook`) into normalized events; signature verification uses a host function so the secret stays out of the plugin.
- Filtering and routing live in the control plane:
  - Ignore events authored by the instance's own bot account.
  - React to submitted reviews, never to individual review comments.
  - Debounce per task (e.g. 30–60 s).
- Routing:
  - PR events whose head branch maps to a task (branch → task mapping in the database, ADR-0025) → that task.
  - Issue comments → the issue's active task, only when the bot account is mentioned.
  - A mention on an issue without an active task → new task.
  - Anything else → recorded on the timeline without waking anyone.
- Waking a task means queuing a short notification message; the agent fetches details through forge tools.

## Consequences

- Routing is deterministic because Dark Factory knows the branches and PRs it works on.
- Event payloads never enter the model context directly.

## Alternatives considered

- **A new task per event**: loses the task's PVC and history.
- **Filtering in plugins**: duplicated per plugin and harder to keep consistent.
