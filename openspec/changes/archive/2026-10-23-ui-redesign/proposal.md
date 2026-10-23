# Proposal

## Why

The iteration-1 web UI is a stack of separate pages in one dark theme: the task list, the detail page and creation are each a full page load away, details sit below a long transcript, and every view styles itself with ad-hoc Tailwind classes. Board members switch between tasks all day, mostly on desktop and often on a phone, so the UI should keep the work queue, the conversation and the task's context in view together, and new views (egress log, approvals inbox, audit log) should look the same without copying markup.

The agreed mockup (design canvas "Dark Factory UI redesign") defines the target.

## What Changes

- **App shell:** a dark navigation rail on desktop (new task, Tasks, Projects, Personas, Credentials, log out) and a bottom bar on phones (Tasks, New task, Log out). Configuration pages stay off the phone navigation.
- **Three-column task workspace on desktop:** task list | transcript with message input | details (status line, PR and issue, project, persona, branch, runs with token usage). Narrower screens drop to two columns with details as a slide-over, phones drill down from list to transcript with details in a bottom sheet.
- **Task list filters:** status filter chips (Active, Waiting, Done, All) and a project filter with search, kept in the URL.
- **Light and dark theme** that follows the system setting; matrix-green accent, Inter and JetBrains Mono (self-hosted).
- **New task composer:** a floating dialog (bottom sheet on phones) with a project chip, one description field, an options menu (linked issue) and an assignee picker, opened from the rail's and bottom bar's **+**. **BREAKING (UI):** the separate title field and the `/tasks/new` page are removed.
- **Task titles without a title field:** a task starts with the first sentence of its description as a placeholder title; the persona sets a title with the new factory tool `set_task_title`; a board member can rename a task, which locks the title. A later "minion" change can add a model-generated title in between; this change only defines the precedence.
- **Design system:** semantic Tailwind tokens for both themes, reusable maud components (buttons, badges, chips, fields, menus, dialogs and sheets, list rows, transcript items) and a documented design system (`docs/design-system.md`) that new views follow.
- **Configuration pages** (projects, personas, credentials) are restyled with the same components; their behavior does not change.

## Capabilities

### New Capabilities
- `web-ui-shell`: the frame every page shares: navigation per form factor, responsive layout rules, light/dark theme, accessibility baseline (contrast, touch targets, keyboard use) and self-hosted assets. The design system itself is a documented convention (design.md, `docs/design-system.md`), not a spec requirement.

### Modified Capabilities
- `task-ui`: task list with filters, three-column workspace, composer instead of the creation form, title display and rename, details panel and sheet, phone navigation.
- `task-management`: a task can be created without a title; title sources and their precedence (user > generated > agent > placeholder); rename locks the title; the assignment prompt asks the persona to set a title.
- `factory-tools`: new tool `set_task_title`.

## Impact

- **Code:** `crates/df-web` (layout, all pages, new `components` module, transcript rendering, styles, assets), `crates/df-db` (title source column, rename, list filters), `crates/df-agent-channel` (tool `set_task_title`, assignment prompt), `contract/mcp/factory-tools.v1.json` (additive).
- **Database:** migration adding `tasks.title_source`; existing tasks become `user`.
- **Assets:** Inter and JetBrains Mono `woff2` (SIL OFL 1.1) and Lucide icons (ISC) vendored under `crates/df-web/assets/`; no CDN.
- **Docs:** new `docs/design-system.md`; `crates/df-web/README.md` points to it.
- **Overlapping changes:** `iter2-cancel`, `iter2-run-resilience` and `iter2-task-volume-lifecycle` also modify `task-ui`; their UI parts (cancel button, resume info, reset workspace) go into the details panel and use the design system components.
