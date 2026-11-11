# Proposal

## Why

A chat plans work in a project, and its runs start from nothing but the persona's prompt, so the board member re-explains the project's priorities, decisions and process in every new chat. ADR-0026 plans a project context that Dark Factory keeps per project. Implementation conventions already live in the repository's `AGENTS.md`. The project context holds the planning knowledge that doesn't belong there. It is injected into every run, so a single prompt-injected run could steer all later ones; for that reason only humans change it, and agents propose changes (ADR-0026 amended, ADR-0040).

## What Changes

- **Project context**:
  - A Markdown text per project, empty by default, at most 32 KiB.
  - Stored in Dark Factory only. It is never synced with `AGENTS.md` and never written into the workspace.
- **Editing**:
  - A "Context" tab on the project settings page (between Security and Integrations, `?tab=context`) with a textarea, saved with "Save project".
  - Board members edit it (`project:context:write` once ADR-0011 is implemented).
  - Changes are audited with before and after values, like other project settings. There is no separate revision history.
- **Injection, chats only**:
  - The system prompt of a chat run is the persona's prompt, then `## Project context` followed by the text, then the chat instructions.
  - An empty context is left out, heading included.
  - The text is read at run start and stays fixed for the run.
  - Task runs never receive it.
- **Proposals by agents**:
  - New approval type `project-context`, raised through the existing `request_approval` with payload `{content}`, the full replacement text, and the single scope `project`.
  - Only chat runs may raise it.
  - It records the version of the context injected into the proposing run.
  - Approving replaces the context in the same transaction. If the context changed since that version, approving fails with "context changed since this proposal", and the request can only be denied.
  - As for every request, the decision is delivered to the chat as a `request.decided` message.
  - A pending `project-context` request doesn't block archiving its chat.
- **Inbox**: a `project-context` request shows the reason and a unified line diff of the current and the proposed text.
- **Chat UI**: the Overview tab links to the project's Context tab.
- **Docs**:
  - ADR-0026 amended;
  - the glossary term Project context;
  - `iteration-3.md` and `agent-playbook.md` (written);
  - a how-to "Keep planning knowledge in the project context".

Out of scope:
- **Project context for task runs**: rejected. Tasks get what they need in their prompt from the chat that creates them, and implementation conventions belong in `AGENTS.md`.
- **Direct writes by agents** (`project:context:write` as an agent grant): rejected (ADR-0026 amended).
- **Revision history and restore**: rejected. The audit log keeps before and after values.
- **Live Markdown preview in the editor**: backlog.
- **Promoting agent memories into the project context**: with agent memory (ADR-0040), backlog.
- **Withdrawing older pending proposals automatically**: rejected. Approving one makes the others stale.

## Capabilities

### New Capabilities
- `project-context`: the per-project context. Covers the text and its limit, editing and audit, injection into chat runs, the `project-context` approval type with its stale check, and how proposals look in the inbox.

### Modified Capabilities
- `chats` (added by `iter3-chats`):
  - "Chat instructions": the project context sits between the persona prompt and the chat instructions.
  - "Archiving a chat": a pending `project-context` request doesn't block archiving.
- `chat-ui` (added by `iter3-chats`): "Details tabs" gets the project context link.
- `factory-tools`: `request_approval` accepts `project-context` from chat runs only.

## Impact

- `crates/df-db`:
  - migration: `projects.context TEXT NOT NULL DEFAULT ''` with a length check; the injected context's version on `runs`;
  - the `project-context` `ApprovalType`;
  - project read and update, audit;
  - `.sqlx/`.
- `crates/df-agent-channel`: `build_assign` adds the context to chat runs and records its version; `request_approval` rejects `project-context` outside chats.
- `crates/df-web`: the Context tab on the project settings page, the diff in request cards, the Overview link in the chat page.
- `contract/mcp/factory-tools.v2.json`: `project-context` in the `request_approval` type enum.
- Dependencies: `similar` becomes a direct workspace dependency (already in `Cargo.lock`).
- `crates/fake-acp-agent`, `crates/df-e2e`: a chat proposes a context, a board member approves it, and the next chat run receives it.
- Depends on `iter3-chats` (task kind `chat`, chat instructions, the archive check, the Overview tab).
- Depends on `ui-model-provider-page` (the tab "Security", the tab script fix for the tab of the first error).
