## Context

- The project settings page (`config_pages/project_page.rs`, archived change `ui-settings-and-task-sort`) is the pattern to follow. It has a back link, a header row with `#save-status` (`role=status`, `aria-live=polite`) and "Save project", and a `tabbed_form(…, TabsStyle::Page)` that syncs `?tab=` through `scripts/tabs.ts`. A save re-renders the form and swaps the heading and status out of band. Creation from the modal answers `HX-Location`.
- Model providers today (`config_pages/providers.rs`):
  - `GET /model-providers/{id}/edit` returns `wider_modal("Edit model provider", provider_form(…))`.
  - `provider_form` uses `tabbed_form(…, TabsStyle::Dialog)` with tabs `general` and `models`.
  - `POST /model-providers/{id}` saves and swaps the list (`config_pages::saved`).
  - `POST /model-providers/refresh` (add or remove a model row, preset changes) re-renders `#form` in create or edit mode.
  - Delete is a row action in the list.
  - Access is `require_board_member`; there is no admin-only part.
- Task details (`task_pages/view.rs`): the description is a `<details>` whose `summary` is `flex`, which removes the browser marker. As a result there is no chevron at all. Runs (`view.rs`) and Dependencies (`task_pages/dependencies.rs`) use `details.group`, a `list-none` summary and `ChevronRight`/`ChevronDown` spans toggled by `group-open:`.
- Transcript: agent messages that are complete go through `markdown::render` (comrak, `escape = true`, syntect fences, link hardening) into `div.md-body`. `user_message` (`components/transcript.rs`) puts the joined visible parts as escaped text in `div.whitespace-pre-wrap` on `bg-bubble-me`. Hidden parts render below it.

## Goals / Non-Goals

**Goals:**
- Editing a model provider works like editing a project: a linkable page in the settings area, with tabs in the URL, that stays open after a save.
- The page uses its width for the Models tab, with a layout settled on the PenPot board.
- Description header with a chevron; user messages highlighted like agent messages.

**Non-Goals:**
- Delete on the settings page: rejected (the list row keeps it, as for projects).
- A page for creating a provider: rejected ("New model provider" stays a modal, as "New project" does).
- Markdown in the queued stack or in the details column's description: rejected (both are short previews).
- Pages for personas and credential references: rejected (short, untabbed forms).

## Decisions

### D1: PenPot first
The design tasks come before the UI tasks, following `docs/design-workflow.md`, on the boards "Config - Model providers" and "Tasks - Details Sidebar" (and the transcript board that holds user messages):
- the provider settings page at 1440 and 390 px: General, Models (many models), model errors, saved status. The board settles how the Models tab uses the width. The proposal to start from: one line per model (ID, wire API, limits, remove) from 1024 px.
- the existing "Model providers · edit, …" boards become "settings page" boards. The "new …" boards stay modals.
- the description header, collapsed and expanded, with the chevron
- a user message with a highlighted fence and a multi-line paragraph, light and dark
- the project settings boards on "Config - Projects" show the tab label "Security", and the boards "Projects · settings page, push checks" and "…, push checks, invalid glob" are renamed to "…, security" (D7)

The user approves the boards in the design session. If the approved layout changes anything in the spec deltas, the deltas are updated before the UI tasks start.

### D2: Shared page frame, provider-specific content
Extract the frame of `project_page.rs` into one helper in `config_pages.rs`. The frame is: back link, header row (heading markup passed in), `#save-status` region, the save button bound to `form="form"`, and the form below. The project page and the provider page both call it. The heading content (key pill and forge icon for projects, name for providers) and the saved message stay per page.

Alternative: copy the 30 lines into the provider page. Rejected, because two copies of the same frame drift apart, and the frame is what makes the two sections consistent.

### D3: Provider page module and routes
- New module `config_pages/providers/page.rs` (`providers.rs` is at 1,019 lines). It holds the `GET /model-providers/{id}` handler, the `?tab=` parsing (`general` | `models`, otherwise General) and the saved response.
- `GET /model-providers/{id}/edit` and the `edit` handler are removed, so that path answers `404`. There is no redirect (pre-MVP).
- `provider_form` gets its `TabsStyle` from the mode: `Dialog` for create, `Page` for edit. The form tab state then follows the existing `Tab` ids, and `Page` adds the hidden `tab` input.
- `update` on success re-renders the form on the posted tab and swaps the heading (name may have changed) and `#save-status` ("Model provider saved") out of band, as `project_page::saved` does. On error, the open tab is the tab of the first error (`models_only(errors)` already decides Models vs General).
- `refresh` keeps its create or edit mode (existing test). In edit mode it now renders `Page` style and keeps the current tab.
- The tab the server opens wins over the URL, and the URL follows it.
  - Today, `init()` in `scripts/tabs.ts` opens the tab named by `?tab=` when `syncUrl` is set. After a failed save from `?tab=models` with an error on General, the swapped-in form opens Models again and hides the error.
  - The project settings page has the same bug (TD-124: after a failed save from `?tab=push-checks` the General error stays hidden).
  - Fix in the shared code: `init()` stops reading `?tab=` and calls `writeUrl()` for the server's tab instead. Reading the URL is redundant, because the server already opens the tab from the query on `GET`.
  - Both pages get the fix.
- `create` answers `HX-Location: /model-providers/{id}` instead of swapping the list.
- List row: the name and the edit icon become links to the page (no `hx-get`/modal).

### D4: Layout and navigation
`Nav::ModelProviderSettings` maps through `settings_section()` to Model providers, so the sidebar marks it. Like `Nav::ProjectSettings`, it hides the bottom bar and the floating create button. Below 768 px the page is a plain full page, reachable from links only (Model providers is not in the phone navigation, unchanged).

### D5: Description chevron
Copy the Runs pattern onto the description `details`: `group` on `details`, `list-none [&::-webkit-details-marker]:hidden` on the summary, a `ChevronRight` span shown while closed and a `ChevronDown` span shown while open. The default stays collapsed, and the three-line clamp and tooltip are unchanged.

### D5a: Project and persona links
- In `details_region` (`task_pages/view.rs`), the "Project" and "Persona" property values become links in the style of the parent link: `/projects/{project_key}` and `/personas`. The key chip and the harness label stay inside the link.
- `TaskDetail` already has `project_key`. The persona link needs no id, so the query is unchanged.
- The bottom sheet on phones renders the same properties, so it gets the links too.

Alternative: open the persona's edit modal through `/personas?edit={id}`. Rejected by the user: the list is enough, and it needs no change on the personas page.

### D6: User messages as Markdown with hard breaks
- `markdown.rs` gets `render_with_hard_breaks(source)`: the same options as `render`, plus comrak's `render.hardbreaks = true`. Both build their options in one shared function, so that the untrusted-input settings cannot diverge.
- `user_message` renders the joined visible parts through it into `div.md-body` inside the bubble. Hidden parts stay plain and muted, as today.
- User text is always complete, so the streaming rule ("Highlighting only completed Markdown") does not apply.
- Mermaid works without extra code, because `scripts/markdown.ts` binds to the rendered fence markup.
- Links in user text become clickable with `rel="noopener noreferrer nofollow"`, and images become links: the same rules as for agent text.
- `.md-body` is checked on `bg-bubble-me` (inline code, links, code block background, margins of the first and last child) and adjusted in `styles/input.css` only where it clashes.

Alternatives:
- Plain Markdown with soft breaks: rejected by the user. Typed line breaks would collapse.
- Keeping runs of blank lines as extra vertical space: rejected. `hardbreaks` only affects soft breaks, and comrak collapses consecutive blank lines into one paragraph break. Keeping them would mean preprocessing the source outside fences. The spec promises line breaks for single newlines and paragraphs for blank lines, which is what comrak does.
- Highlighting only the fences: rejected. It needs a custom splitter and would differ from agent messages.

### D7: Rename "Push checks" to "Security"
- Since `iter3-hidden-paths`, the tab holds the push checks and the hidden paths. Hidden paths are not a push check, so the tab is renamed "Security".
- The tab id in `config_pages.rs` becomes `security`, so the URL is `?tab=security`. There is no alias for `push-checks`: an unknown value already opens General (pre-MVP).
- `config_pages/push_policy.rs` keeps its name, because it still renders the project's push policy. Only its doc comment and the comments that name the tab change.

Alternative: keep "Push checks". Rejected, because the label hides the hidden paths.

### D8: Adapter command with shell quoting (TD-60)
- `validate_project` parses the field with `shlex::split`. `None` (an unclosed quote) is a validation error at `adapter_command`. An empty or whitespace-only field gives no arguments, as today.
- `project_values` shows the stored arguments with `shlex::try_join`, so an argument with whitespace is quoted and survives the next save. `try_join` only fails on a NUL byte, which `shlex::split` never produces. In that case the field falls back to the arguments joined with spaces.
- `shlex` 2.0.1 is already in `Cargo.lock` as a transitive dependency. It becomes a direct dependency of `df-web` through `[workspace.dependencies]` (skill `rust`).
- The field stays a single-line text input. The hint mentions quoting.
- The stored format (a list of arguments) is unchanged, so there is no migration.

Alternative: one argument per line in a textarea. Rejected by the user in favor of the familiar shell syntax.

## Risks / Trade-offs

- [Markdown syntax in plain user text changes its look: `*`, `_`, `#` at a line start, and lists] → Accepted. It is the same behavior as agent messages and request texts, and hard breaks keep the line structure.
- [`TabsStyle::Dialog` is left with one user, the "New model provider" modal] → Accepted. No change to `tabbed_form`.
- [Old `/model-providers/{id}/edit` bookmarks break] → Accepted pre-MVP. The 404 is covered by a test.
- [Old `?tab=push-checks` links open General] → Accepted pre-MVP. Covered by a test.
- [An adapter command with a literal quote or backslash, saved before the change, is parsed differently on its next save] → Accepted pre-MVP. On display it is quoted by `try_join`, so saving it unchanged keeps it.

## Open Questions

- Layout of the Models tab on the wide page: settled in the PenPot task (D1).
