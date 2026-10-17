# ADR-0028: Integration plugins as capability interfaces

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0002](0002-wasi-plugins-wasmtime-component-model.md), [ADR-0018](0018-plugin-packaging-versioning-config.md)

## Context

ADR-0002 and ADR-0018 planned one WIT world per extension point (`forge`, `issue-tracker`). Iteration 1 needs both PR operations and issue comments from Forgejo, which hosts both, while trackers like Jira only provide issues.

## Decision

- One package `darkfactory:integrations@0.1.0` with a shared `types` interface and capability interfaces `pull-requests` (`create`, `find-open`, `get`, `update`) and `issues` (`get`, `add-comment`).
- Worlds: `pull-request-provider`, `issue-provider` and `forge` (both). The control plane detects capabilities from a component's exports.
- `find-open` makes PR creation idempotent and supports attaching existing PRs (ADR-0025).
- `draft` is a forge-neutral flag; plugins emulate it where the forge lacks native drafts.
- Imports pinned to `wasi:*@0.2.12` (Wasmtime 46) and `wasi:config/store@0.2.0-rc.1` (implemented by `wasmtime-wasi-config`).
- The host links standard WASI with no capabilities granted (no preopens, environment or sockets), so `wasi:http` is the only way out.

## Consequences

- One Forgejo component covers both capabilities; issue-only trackers implement less.
- The control plane needs a Wasmtime release that provides WASI 0.2.12; guests built against older 0.2.x still link.
- Contract: `contract/wit/integrations/integrations.wit`.

## Alternatives considered

- **Separate packages per extension point**: shared types need a third package; a forge plugin would implement two packages anyway.
