# ADR-0036: Code review stays in the forge

- **Status:** Accepted
- **Date:** 2026-10-17
- **Related:** [ADR-0011](0011-authentication-authorization-personas.md), [ADR-0015](0015-webhook-filtering-and-routing.md), [ADR-0025](0025-pr-and-branch-handling.md), [ADR-0035](0035-ui-form-factors-phone-for-core-flows.md)

## Context

Agents produce changes as pull requests in the forge (ADR-0025), and the forge already has everything for review: line comments, review threads, approvals, suggestions, CI status. With the workspace changes view (F8) Dark Factory shows diffs itself, which invites adding review features to it — duplicating the forge and splitting feedback across two places.

## Decision

- **Feedback on changes is out of scope for Dark Factory.** No line comments, review threads, change approvals or suggested edits in the factory UI.
- Review happens in the forge, on the pull request. Dark Factory routes review events back to the task (ADR-0015), so a submitted review wakes the agent.
- Agents reviewing other agents' work (agent playbook, pattern 2.2) review in the forge too, through the forge integration.
- Views of changes in Dark Factory (F8) are read-only and link to the pull request for feedback.
- Free-form messages to a task or chat stay in Dark Factory — they instruct the agent; line-anchored feedback on code belongs in the forge.
- Approvals in Dark Factory are about agent actions (ADR-0011), not about code changes.

## Consequences

- One place for review history, next to CI, merge rules and branch protection; reviews stay visible to anyone using the forge, with or without Dark Factory.
- The forge integration has to carry review data well: PR comments and review events (planned WIT additions, ADR-0015).
- Reviewing on the phone happens in the forge's own UI or app, not in Dark Factory (ADR-0035).

## Alternatives considered

- **Review inside Dark Factory, synced to the forge**: two-way sync of threads is complex and error-prone.
- **Review inside Dark Factory only**: hides review history from the forge and duplicates what it already does well.
