# ADR-0035: UI form factors: one responsive web UI, phone for core flows

- **Status:** Accepted
- **Date:** 2026-10-17
- **Related:** [ADR-0004](0004-server-rendered-ui-maud-htmx-alpine.md), [ADR-0011](0011-authentication-authorization-personas.md), [ADR-0026](0026-chats-dependencies-project-context.md), [ADR-0034](0034-task-hierarchy-subtask-branches-and-merging.md), [ADR-0037](0037-tailwind-css-standalone-binary.md)

## Context

The UI so far implicitly assumed a desktop browser. Many interactions with agents are short and happen away from the desk: starting a task, checking what an agent did, answering its question, continuing a chat. Others need a large screen: reading diffs and file trees, configuring personas and projects, analysing costs.

## Decision

- **One responsive web UI**, no native app and no separate mobile site. The same server-rendered pages (ADR-0004) adapt to the screen width.
- **Phone-supported flows** — designed and tested on a phone-sized screen:
  - task list with status, and starting a task (create form);
  - task detail: reading the live transcript, sending messages, answering questions and approvals (`AwaitingInput`), links to PR and issue;
  - chats with agents (ADR-0026);
  - task tree navigation (ADR-0034).
- **Desktop-only flows** — reachable on a phone, but not designed for it and left out of the mobile navigation: workspace changes view (F8), live egress log, configuration of projects, personas, skills, tools and credentials, audit and cost views.
- Layout rules for the phone-supported pages: mobile-first CSS, touch-sized targets, no hover-only interactions (tool-call details expand on tap), long code blocks scroll horizontally inside their block instead of widening the page.
- **Robust live updates:** phones sleep and switch networks; SSE reconnects and replays from `Last-Event-ID` (ADR-0004), and the message form keeps its draft across reconnects.
- *(Later)* An installable web app manifest, and push notifications (Web Push) for questions and completed tasks.

## Consequences

- Every new page states which group it belongs to; phone-supported pages are checked at phone width as part of the end-to-end demo.
- The OIDC session must last long enough that opening the UI on a phone doesn't mean logging in every time (e.g. refresh tokens with the OIDC login of ADR-0011).
- Tailwind CSS (ADR-0037) provides the mobile-first breakpoints for the responsive layout.

## Alternatives considered

- **Native or cross-platform app**: a second client to build and ship; the server-rendered UI already covers the phone flows.
- **Separate mobile site**: duplicates templates and routes.
- **Desktop-only UI**: rules out the short interactions that are most useful away from the desk.
- **Full feature parity on the phone**: diff and configuration screens would be compromised for a use that isn't needed.
