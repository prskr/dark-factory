# Design

## Context

- **Run prompt today**:
  - `load_run_context` (`crates/df-agent-channel/src/assign.rs`) reads `personas.system_prompt`, and `build_assign` sends it as `Assign.harness.system_prompt`.
  - The runtime writes it to `$HOME/.claude/CLAUDE.md` once per run.
  - `Assign` is identical for every resend of a run.
- **Chat runs** (`iter3-chats`, design D3): `build_assign` appends `CHAT_INSTRUCTIONS` to the persona prompt for `kind = 'chat'`.
- **Approvals**:
  - Types implement `ApprovalType` (`crates/df-db/src/requests.rs`): payload validation, fingerprint, scopes, `lock`, and `apply` in the decision's transaction.
  - Every request belongs to a task, and `raised_by` names the run.
- **Project form**: `tabbed_form` (General, Push checks) in `config_pages.rs`. `push_policy.rs` is the module pattern to copy: `panel`, `validate`, `FIELDS`. The open tab is chosen by the server (`tabs("general")`).
- **Authorization**: there is no permission model in code yet. Board members do everything, so `project:context:write` stays the documented target of ADR-0011.

## Goals / Non-Goals

**Goals:**
- One place that composes a chat's system prompt.
- A stale check that can't be bypassed by what the agent sends.

**Non-Goals:**
- Optimistic locking for the project form. Two board members saving the form at once still means last write wins, as for every other project field.

## Decisions

### D1: Storage and version

The migration adds `projects.context TEXT NOT NULL DEFAULT ''` with `CHECK (octet_length(context) <= 32768)`, plus `projects.context_version BIGINT NOT NULL DEFAULT 0`.
- The version is bumped only when the text actually changes, whether through the form or an approval.
- `projects.updated_at` is not used, because editing the project name would make every pending proposal stale.

The limit `PROJECT_CONTEXT_MAX_BYTES = 32 * 1024` is a constant in `df-core`. It is checked by the form validation, by the approval payload validation, and by the DB check as the last line of defence.

The audit trail uses the existing `project.update` entry with `context` in `details.before` / `details.after`. Approving writes the same entry, with the deciding board member as actor.

### D2: Recording the injected context

`runs.project_context TEXT NULL` and `runs.project_context_version BIGINT NULL` are written once, when `build_assign` first composes `Assign` for a chat run. Both are read in one query from `projects`. Task runs leave them NULL. A resend after a reconnect uses the stored text, so `Assign` stays identical for the run even if the context is edited meanwhile.

Alternative: store only the version and look the text up again on resend. That breaks identical resends once the context changes, or needs a lookup in the audit log. Storing up to 32 KiB per chat run is the simpler choice, and chat runs are few.

### D3: Composing the chat prompt

`build_assign` for `kind = 'chat'`:

```
<persona prompt>

## Project context

<context>

<CHAT_INSTRUCTIONS>
```

The middle block is left out when the context is empty. This lives in one helper, `chat_system_prompt(persona, context)`, which replaces the plain concatenation from `iter3-chats` D3. `CHAT_INSTRUCTIONS` gains one line: changes to the project context are proposed with `request_approval`, type `project-context`, with the full new text. Task runs are unchanged.

### D4: The `project-context` approval type

`ProjectContextApproval` in `crates/df-db/src/project_context.rs`:

| Member | Behavior |
|---|---|
| payload | `{content: string}`, at most 32 KiB, checked at raise time. |
| fingerprint | SHA-256 hex of `content` (`sha2` is already a workspace dependency). |
| scopes | `[Scope::Project]` |
| raise | The control plane rejects the request unless the raising run's task is a chat. It adds `base_version` = `runs.project_context_version` of the raising run to the stored payload. The agent can't supply or override it. |
| `lock` | Locks the project row (`SELECT … FOR UPDATE`). |
| `apply` | `UPDATE projects SET context = $content, context_version = context_version + 1 WHERE id = $p AND context_version = $base_version`. Zero rows means `DbError` "context changed since this proposal": the decision isn't stored and the request stays pending (spec `agent-requests`, "Approved actions are applied by the control plane"). |

The agent-facing check (chat only, size) happens in the `request_approval` handler before the request is stored, and comes back as a tool error. The `factory-tools.v2.json` enum gains `project-context`.

Alternative: a separate tool `propose_project_context`. It would duplicate `request_approval` and need a `CHAT_TOOLS` change.

### D5: Archive check

The `iter3-chats` query "open approval requests of this chat" gets `AND approval_type IS DISTINCT FROM 'project-context'`. Decisions on an archived chat queue `request.decided` messages like child-ended messages. No run starts until the chat is unarchived.

### D6: Diff in the inbox

The request card for `project-context` renders `similar::TextDiff::from_lines(current, proposed).unified_diff().context_radius(3)`:
- inside a `<pre>`, one span per line with a class for added, removed and context lines;
- maud escapes the text.

The diff is computed at render time against the current context. A stale proposal therefore shows what approving would overwrite, and the Approve button is disabled with "context changed since this proposal" when `context_version` != `base_version`. `similar` becomes a workspace dependency with default features off (it is already in `Cargo.lock`).

### D7: Context tab

`crates/df-web/src/config_pages/project_context.rs` follows `push_policy.rs`:
- `panel` (textarea, about 20 rows, a hint to keep implementation conventions in `AGENTS.md`), `validate` (size) and the field;
- `ProjectForm` flattens it;
- `save_project` bumps `context_version` only when the text differs.

`GET /projects/{key}/edit?tab=context` passes `"context"` as the initial tab, and the chat Overview link points there.

## Risks / Trade-offs

- [A chat with stale context proposes a text based on an old version] → D4 rejects the approval. The chat learns about it from the `request.decided` message after the human denies, and re-proposes in a later run that sees the new text.
- [Prompt injection through an approved context] → Only a human approves, and the diff shows exactly what changes. Task runs never receive the text, so untrusted repository content read by a task can't reach it except through a chat proposal that a human reviews.
- [32 KiB per chat run is stored on `runs`] → Chat runs are few, compared with task runs. Revisit if storage shows up.

## Migration Plan

One additive migration (two columns on `projects`, two on `runs`). There is no backfill: existing projects start with an empty context. Regenerate `.sqlx/`.
