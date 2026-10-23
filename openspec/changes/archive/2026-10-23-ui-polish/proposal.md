# Proposal

## Why

After `ui-redesign`, the web UI still differs from the agreed mockup in several places (rail, composer, list header), and the review of the full design canvas "Dark Factory UI redesign" turned up gaps: the transcript reads like a log rather than a conversation, queued messages cannot be corrected, configuration pages are separate forms without overview, and the phone has no way to reach projects. This change closes those gaps before the UI is built on further.

## What Changes

- **Rail as in the mockup:** icon-only with the factory logo, the mockup's icons and a green active bar; section names become tooltips.
- **Logo:** the factory mark becomes an SVG asset, used in the rail and as favicon.
- **Phone navigation:** bottom bar Tasks / + / Projects, where + creates whatever the screen lists (task or project); a profile avatar top right opens a menu with name, role and Log out. Projects joins the phone navigation. **BREAKING (UI):** Log out leaves the bottom bar.
- **Task list header:** the project filter moves into the "Tasks" title row (desktop); on phones the list filters by status only and is filtered by project from the Projects screen.
- **Conversation transcript:** messages as bubbles (board member right, agent left), avatars on the last message of a group, name and time under the bubble, inline code chips, borderless expandable tool rows and run dividers.
- **Queued messages:** shown as a stack above the message input instead of in the transcript; each can be edited (in the message input) or removed until a turn picks it up.
- **Composer as in the mockup:** no solid modal frame, chips above a rounded card, icon submit button.
- **Rename:** explicit Save and Cancel next to the inline title field.
- **Collapsible details column** on desktop, remembered per browser.
- **Projects overview:** table with Queued / Running / Waiting / Done counters per project that link to the filtered task list, and a forge icon per repository; the project form shows which forge integration the repository and issue tracker URLs map to while typing.
- **Personas as cards.**
- **Configuration forms as modals** (phone: full-height sheet) with validation errors inside the modal. **BREAKING (UI):** the separate `/projects/new`, `/projects/{key}`, `/personas/new`, `/personas/{id}`, `/credentials/new`, `/credentials/{id}` pages go away.
- **Empty states** for the task list, the workspace without a selected task and the three configuration lists.
- **Manual browser check** carried over from `ui-redesign` (task 8.2).

Non-goals: details as tabs (sub-tasks, artifacts), a sub-task chip in the task list, autocomplete of Secret names and keys in the credential form, dark mockups of the configuration pages (they follow the tokens).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `web-ui-shell`: rail look and tooltips, logo and favicon, phone bottom bar (Tasks / context-aware create / Projects), profile menu, empty states as a shared pattern.
- `task-ui`: conversation transcript, queued-message stack with edit and remove, rename controls, collapsible details column, task list header and phone filtering, composer look, empty task list and workspace.
- `task-management`: a queued message can be edited or removed while it is undelivered.
- `configuration-management`: project overview with task counters and forge indicator, forge match feedback in the project form, persona cards, forms as modals, empty states, Projects on the phone navigation.

## Impact

- **Code:** `crates/df-web` (shell, components, task pages, config pages, transcript rendering, styles, new logo asset), `crates/df-db` (forwarding claim, edit and remove of queued messages, per-project status counts), `crates/df-agent-channel` (forward queued messages only between turns), `crates/dark-factory` (pass the configured forge integration to the web UI).
- **Routes:** `PATCH` and `DELETE /tasks/{key}/messages/{id}`, a forge-match fragment for the project form, modal fragments for configuration forms; the full-page configuration form routes are removed.
- **Database:** migration adding `task_messages.forwarded_at`; `.sqlx/` metadata for new and changed queries.
- **Runtime:** no change; `factory-agent` already deduplicates prompts by id.
- **Docs:** `docs/design-system.md` gains modal/sheet, bubble, queued stack, empty state, forge icon and profile menu.
