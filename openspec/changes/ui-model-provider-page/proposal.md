## Why

`ui-settings-and-task-sort` turned the project edit modal into a settings page inside the settings area. Model providers are still edited in a modal, so the two settings sections behave differently. The modal also limits the Models tab: each model row has an ID, a wire API and several limit fields, and at 896 px they wrap. The task UI has two small defects. The "Description" header in the details column has no chevron, although a click expands it. User messages are plain text, so code a user pastes is not highlighted, while the same code in an agent message is.

## What Changes

- **BREAKING (UI):** editing a model provider moves from a modal to a **model provider settings page** at `/model-providers/{id}`, built like the project settings page:
  - From 768 px it sits in the settings area, next to the settings sidebar with Model providers marked current. Below 768 px it is a full page.
  - It has a back link "Model providers", a header with the provider name, the tabs General and Models, and one "Save model provider" button.
  - The tab is part of the URL (`?tab=models`; General is left out).
  - Saving keeps the page open and confirms with "Model provider saved". A validation error opens the tab of the first error.
  - The list row's name and edit action link to the page. Delete stays in the list row.
  - "New model provider" stays a modal. After creation, the page of the new provider opens.
  - `/model-providers/{id}/edit` is removed. Old links break; that is acceptable pre-MVP.
  - The PenPot design session settles the layout of the page, in particular how the Models tab uses the extra width.
- **BREAKING (UI):** the project settings tab "Push checks" is renamed "Security", because it holds the push checks and the hidden paths. Its URL value changes from `?tab=push-checks` to `?tab=security`. Old links open General.
- **Adapter command with shell quoting (TD-60):** the project's adapter command field is parsed with shell-style quoting and shown quoted, so an argument containing a space (`'--config=/workspace/my config.json'`) survives an edit. An unclosed quote is a validation error. The stored format (a list of arguments) is unchanged.
- **Description chevron:** the collapsible "Description" header in the task details column gets the same chevron as Runs and Dependencies: right when closed, down when open.
- **Project and persona links:** in the task details, the project value links to the project's settings page and the persona value to the persona list (personas have no settings page).
- **Markdown in user messages:** the visible text of a user message in the transcript is rendered with the same Markdown renderer and the same untrusted-input rules as agent messages. Fenced code is highlighted and Mermaid fences are drawn. A single newline stays a line break, so typed lines keep their shape. Blank lines separate paragraphs as in agent messages: several in a row count as one. Hidden blocks ("Instruction for the agent") and the queued message stack stay plain text.

Out of scope:
- A delete action on the provider settings page: rejected (delete stays in the list row, as for projects).
- Turning "New model provider" into a page: rejected (projects keep a creation modal too; both stay consistent).
- Markdown in the queued message stack and in the task description of the details column: rejected (both are short previews; the description is cut after three lines).
- Settings pages for personas and credential references: rejected (their forms are short and untabbed; the modal fits).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `model-providers`: new requirement for the model provider settings page (page, tab in the URL, saving).
- `configuration-management`: "Configuration forms as modals" covers creating a model provider, which opens its settings page. "Form tabs on phones" covers the model provider settings page. "Saving the project settings page": after a validation error the URL names the tab with the error (bug fix in the shared tab script; fixes TD-124). "Projects": the adapter command uses shell-style quoting (TD-60). "Project settings page" and "Project settings tab in the URL": the tab "Push checks" becomes "Security" (`tab=security`).
- `event-subscriptions`: "Lifecycle events tab position" names the tabs "General", "Security" and "Integrations".
- `task-ui`: new requirements for the chevron of the collapsible description and for the project and persona links in the task details.
- `markdown-rendering`: new requirement for Markdown in user messages (hard line breaks, plain hidden blocks and queued stack).

## Impact

- Code (`df-web`):
  - `config_pages/providers.rs`: routes (`GET /model-providers/{id}` page, `/edit` removed), `create` sends `HX-Location`, `update` re-renders the page form with a save status, `refresh` keeps the page tab style when editing, list row links.
  - new `config_pages/providers/page.rs` (the page; `providers.rs` is at 1,019 lines)
  - `config_pages/project_page.rs`: the page frame (back link, header row, status region, save button) is shared with the provider page
  - `layout.rs`: `Nav::ModelProviderSettings` mapped to the Model providers section, no bottom bar or floating button, like `Nav::ProjectSettings`
  - `task_pages/view.rs`: chevron on the description summary; project and persona values as links
  - `markdown.rs`: a render variant with hard line breaks
  - `components/transcript.rs` `user_message`, `transcript/mod.rs`: Markdown body for visible parts
  - `styles/input.css`: `md-body` inside the user bubble (code block, links, inline code on `bg-bubble-me`), if needed
  - `config_pages.rs`: id and label of the Security tab; doc comments in `config_pages.rs`, `project_page.rs` and `push_policy.rs`; adapter command parsed with `shlex::split` and shown with `shlex::try_join` (`validate_project`, `project_values`)
- Dependencies: `shlex` (already in `Cargo.lock` as a transitive dependency) becomes a direct dependency of `df-web`.
- Tech debt: TD-60 and TD-124 are fixed and removed from `docs/tech_debt.md`.
- Tests: `config_pages/providers/tests.rs`, `components/transcript.rs`, `transcript/tests.rs`, `task_pages/tests/detail.rs`, `markdown.rs`, `config_pages/tests/project_page.rs`, `config_pages/tests/integrations_tab.rs`, `config_pages/push_policy/tests.rs`, the project form tests in `config_pages/tests/`; e2e in `crates/df-e2e/tests/ui.rs`.
- Design: PenPot boards for the provider settings page (General, Models, model errors, 1440 and 390 px), the description header with chevron, and a user message with a code block; the "Security" label on the project settings boards; mapping rows in `docs/design-workflow.md`.
- Docs: `docs/content/using/model-providers.md`, `docs/content/using/tasks.md`, `docs/content/using/setup.md`, `docs/content/using/hidden-paths.md`, `docs/content/security/git-proxy.md`.
- No database, API, configuration or Helm change.
