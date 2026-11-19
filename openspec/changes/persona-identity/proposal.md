# Proposal: persona-identity

## Why

Every commit a sandbox makes carries one instance-wide commit identity (`agent.commitIdentity`). As a result, neither the forge history nor the UI shows which persona did the work, and per-persona commit signing (the follow-up change `persona-commit-signing`) has nothing to bind a key to. Pre-MVP is the cheapest time to make identity a mandatory part of every persona (ADR-0062).

## What Changes

- Every persona has a mandatory **persona identity**: handle, display name, commit email and a locally generated avatar.
- **BREAKING**: the handle (lowercase `[a-z0-9-]`, unique) replaces the unique persona name as the persona's identifier. `create_task` takes `persona` as a handle, and `list_personas` returns handle, display name and description, ordered by handle. The display name is no longer unique.
- The commit email is unique across personas.
- Commits use the run's persona identity: `Assign.workspace.commit_identity` and the gateway's Git route carry it. The harness's commits and factory-agent's WIP snapshot commits both use it. `df:identity` checks against it. An identity edit applies to runs started after it; an active run keeps the identity it started with.
- **BREAKING**: the instance setting `agent.commitIdentity.{name,email}` (Helm `agent.commitIdentity`) is removed, as are factory-agent's built-in fallback author values. An `Assign` without a commit identity is rejected.
- The avatar is derived from the persona's id: initials on a color, rendered by the server. There is no upload, no stored image and no external request.
- The display identity (avatar, display name, `@handle`) is shown on the persona cards and form, as the agent's author in the transcript, on task list rows and in the task details.
- A migration fills identities for existing personas: handle = slug of the name (`-2`, `-3` on collisions), display name = name, email = `<handle>@dark-factory.localhost`.
- Unchanged: forge actions (pushes, pull requests, comments, merge commits) keep the single bot account per project credential.

Out of scope:
- Routing `@handle` mentions in forge comments or chats to a persona: roadmap backlog.
- A forge account per persona: rejected (ADR-0062).
- Commit signing: follow-up change `persona-commit-signing` (roadmap iteration 2 row "Commit signing without keys in the sandbox").
- Persona avatars in the chat UI: the in-flight change `iter3-chats` picks up the shared avatar component.

## Capabilities

### New Capabilities
- `persona-identity`: what a persona identity consists of, its validation and uniqueness, the generated avatar, which commits carry it, and the migration of existing personas.

### Modified Capabilities
- `configuration-management`: the persona form and cards gain handle, display name, email and avatar. The handle replaces the unique name.
- `factory-tools`: `create_task` addresses a persona by handle; `list_personas` returns handle, display name and description.
- `run-orchestration`: `Assign` carries the run's persona identity instead of the instance's commit identity.
- `push-checks`: `df:identity` compares against the run's persona identity.
- `workspace-lifecycle`: rewrites of commits not authored by the run's persona are rejected.
- `task-ui`: the transcript shows the persona's avatar and display name for agent messages; task rows and task details show the persona.

## Impact

- **Database** (`df-db`): `personas` gains `handle` (unique), `display_name`, `commit_email` (unique). `name` is dropped after the backfill. Migration and `.sqlx` metadata are regenerated. `catalog.rs` queries and `persona_id_by_name` change to work by handle.
- **Core** (`df-core`): persona domain types, handle and email validation, `agent.commitIdentity` removed from `config.rs`.
- **Orchestrator / agent channel**: `run_context.rs` loads the identity, `assign.rs` fills `commit_identity` from the persona, the delegation tools go by handle.
- **Gateway control**: the snapshot's `GitRoute.commit_name`/`commit_email` come from the run's persona. The gateway code is unchanged.
- **factory-agent**: `DEFAULT_COMMIT_AUTHOR_*` is removed and a missing identity is an error.
- **Web** (`df-web`): persona form and cards, an avatar component, the transcript author, task rows and details.
- **Helm**: `agent.commitIdentity` values are removed.
- **Seeds / e2e** (`df-e2e`, demo data): personas get handles and emails, and e2e commit assertions change.
- **Docs**: persona configuration reference and a how-to in `docs/content`, the configuration reference without `agent.commitIdentity`, `product/agent-playbook.md`, `design/agent-run.md` §11.1, and a roadmap backlog row for mention routing.
