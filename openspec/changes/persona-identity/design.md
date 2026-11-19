# Design: persona-identity

## Context

- `personas` (`df-db`) has `name UNIQUE`, `description`, `system_prompt`, `harness`, `model_provider_id`, `model` and no identity columns.
- Factory tools look personas up by name (`persona_id_by_name`, `delegation.rs`).
- The commit identity comes from instance config `agent.commitIdentity` (`df-core/src/config.rs`). It reaches the sandbox through `Assign.workspace.commit_identity` (`assign.rs`) and the gateway through `GitRoute.commit_name`/`commit_email` (`df-gateway-control/src/snapshot.rs`).
- factory-agent writes the identity into the runtime-owned `.git/config` (`git.rs`, `rewrite_git_config`). It falls back to `DEFAULT_COMMIT_AUTHOR_*` when the identity is missing.
- The transcript already shows an avatar for agent messages. That avatar is generic.

Motivation: see `proposal.md`. Requirements: see `specs/`.

## Goals / Non-Goals

**Goals:**
- One source for a run's commit identity, shared by `Assign` and the gateway's Git route, so the sandbox and `df:identity` always agree.
- Handle and email uniqueness enforced by the database, not only the form.
- An avatar that needs no storage and no network access.

**Non-Goals:**
- Routing `@handle` mentions to a persona → roadmap backlog.
- Forge accounts per persona → rejected (ADR-0062).
- Commit signing → change `persona-commit-signing`.
- Chat UI avatars → `iter3-chats` uses the avatar component from this change.
- Uploaded avatar images → rejected (decided in the grilling session).

## Decisions

### D1: Freeze the identity on the run

When a run is created, the persona's display name and commit email are copied onto the run row (`runs.commit_name`, `runs.commit_email`, both NOT NULL). `Assign` and the gateway snapshot both read the identity from the run, not from the persona.

- Creation is the run's start: the scheduler inserts the run row in the same transaction as the task's `RunStarted` event (`runs::start_queued_runs`, which also sets `started_at`). `pending` is the first state of a started run, not a queue, so "at the run's start" and "at creation" are the same moment.

- Why: the gateway snapshot is rebuilt continuously. If it read the persona live, an identity edit during a run would make the snapshot expect the new email while the sandbox keeps committing with the old one, and `df:identity` would reject every push. Freezing on the run gives the spec's "identity at the run's start" in one place.
- Alternative: read the persona live and reject identity edits while runs are active. Rejected: it blocks edits for long-running chats, and it needs a check on every save.

### D2: Rename `name` to `display_name` and add `handle`

The migration:
1. Renames `name` to `display_name` and drops its unique constraint.
2. Adds `handle` and `commit_email`.
3. Backfills both in a PL/pgSQL `DO` block that walks the personas ordered by `created_at, id`. The slug comes from `regexp_replace` on `lower(display_name)`, cut to 39 characters, with `persona` when the result is empty. If a persona already processed holds the slug, the block tries `-2`, `-3`, … and cuts the slug to `39 - length(suffix)` characters (trailing `-` trimmed) before appending, until the handle is free. Checking against every handle assigned so far, not per slug group, keeps `Ada`, `ADA!` and `Ada-2` distinct (`ada`, `ada-2`, `ada-2-2`). The email is `<handle>@dark-factory.localhost`. The display name is the old name with control characters, `<` and `>` removed, trimmed and cut to 100 characters, or the handle when nothing is left. Today's form only requires a non-blank name, so such names can exist.
4. Sets both columns NOT NULL with unique constraints, plus a CHECK on the handle pattern.

- Why: renaming keeps the existing values and the audit history readable. Uniqueness in the database also catches concurrent saves, and the form maps a unique violation to a field error naming the other persona.
- The handle CHECK duplicates the validation in `df-core` on purpose: the database is the last line for data that tools read.

### D3: Persona lookup by handle only

`persona_id_by_name` becomes `persona_id_by_handle`. `create_task` matches the handle exactly. It does not fall back to display names, which are not unique and would make delegation ambiguous. The error for an unknown persona tells the agent to use the handle from `list_personas`.

### D4: Validation in `df-core`

A `PersonaIdentity { handle, display_name, commit_email }` value type with constructors that validate:
- handle: the regex from the spec;
- display name: length, and no control characters, `<`, `>` or newline, because these would corrupt `user.name` in git config and commit headers;
- email: one `@`, no whitespace, `<`, `>` or `,`, at most 254 characters, lowercased.

The web form and the DB layer use the same type. No new dependency: the checks are character tests, not a full RFC 5322 parser. An address the forge rejects shows up when the operator registers it.

### D5: The avatar is a server-rendered component

The avatar is a `df-web` partial that renders a `<span role="img" aria-label="…">` with the initials. Its CSS background class comes from a fixed palette of about 8 tokens, picked by a stable hash of the persona id: FNV-1a written inline, because `DefaultHasher` is not stable across Rust releases.
- The form preview is updated by Alpine, which already ships: initials from the input, color from the id. A new persona uses a neutral color until it is saved.
- Alternative: a generated SVG identicon. Rejected: it means more code, and initials are easier to read in small rows.

### D6: Remove the fallbacks

- `agent.commitIdentity` is removed from config, Helm values and docs.
- factory-agent drops `DEFAULT_COMMIT_AUTHOR_*` and fails the run before preparing the workspace when `commit_identity` is missing or has an empty field.
- The proto field stays as it is. Only its comment changes to "the run's persona identity".

### D7: The transcript reads the author live from the persona

Transcript events already carry `persona_id`. The UI resolves the display name, handle and avatar from the current persona at render time, so renaming a persona also updates old transcripts. Commits stay frozen, per D1: Git history is immutable, a UI label is not.

## Risks / Trade-offs

- [Placeholder emails `@dark-factory.localhost` show up in real commits after the upgrade] → The how-to tells operators to set real emails. Pre-MVP, so there are no production installs.
- [A persona edit during a run takes effect only at the next run (D1), which may surprise] → The persona form says so below the email field.
- [Agents or routines that refer to personas by display name break] → Routines store `persona_id`. The persona text in agent prompts comes from `list_personas`, which now returns handles. `agent-playbook.md` is updated.
- [The handle CHECK in SQL and the validation in Rust can drift] → One test feeds the same invalid handles to both.

## Migration Plan

1. Drain before upgrading: let active runs end or cancel them. An active run can hold unpushed commits with the old instance identity, and `df:identity` would reject them against the new one. The upgrade guide says so.
2. One migration: the D2 steps, plus `runs.commit_name`/`commit_email`. Existing runs are backfilled with the persona's new identity. The old instance identity lives in the config file, not the database, so the migration cannot copy it; after a drain no run needs it.
3. Deploy the control plane, gateway and runtime image together (one release). The wire format is unchanged. Only a new runtime against an old control plane that sends an empty identity would fail runs, and they ship together.
4. Rollback: none beyond restoring the database backup. That is acceptable pre-MVP.

## Open Questions

- None.
