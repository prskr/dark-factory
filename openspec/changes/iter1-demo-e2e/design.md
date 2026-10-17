# Design

## Context

All workstreams are applied and deployed with `//deploy:dev` (`iter1-build-and-deploy`). The UI requires an OIDC login, which is impractical to automate reliably; every UI handler calls the same `df-db` task-management operations as the test would.

## Goals / Non-Goals

**Goals:**
- One command runs the full flow against the dev instance with the fake agent, repeatable and isolated per run.
- A manual checklist covers what automation doesn't: the browser/phone UI and real Claude Code.

**Non-Goals:**
- CI execution (no CI integration environment yet, ADR-0024), browser automation, load tests.

## Decisions

### D1 — Drive below the UI, observe through public surfaces
The test connects to the dev database with the application role (URL from env), creates the task and queues messages with `df-db` task operations, and observes through the database (task status, runs, transcript rows) and the Forgejo API (branch, PR, comments). *Alternative:* Playwright against the UI with a Keycloak test user — brittle and slow; the UI is covered by `df-web` handler tests plus the manual checklist.

### D2 — Isolation
Each test run creates its own project with a random key (`E<4 random letters>`) against a fresh Forgejo test repository (created through the Forgejo API with the bot token, deleted afterwards unless `E2E_KEEP=1`). Persona "e2e" uses the test project image and `fake-acp-agent --script /opt/harness/scripts/demo.yaml`.

### D3 — Demo script
`demo.yaml`: new session — say, plan, `shell` commit, `mcp update_pull_request`, `mcp comment_on_issue`, end `end_turn`; loaded session — say, `shell` commit adding a test, `mcp complete_task`, end `end_turn`. A second script `rate-limit.yaml` for the rate-limit scenario.

## Risks / Trade-offs

- [Flaky cold starts (image pulls)] → generous timeouts (5 min per run) and pre-pulled images via Harbor's cache.
- [Test residue in Forgejo] → cleanup in a drop guard; keep only on failure or `E2E_KEEP=1`.
