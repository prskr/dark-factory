## 1. Design (PenPot, design D1)

- [ ] 1.1 PenPot (page "Config - Model providers"): turn the "Model providers · edit, …" boards into the model provider settings page:
  - settings sidebar with Model providers current, back link "Model providers", header with the name, horizontal tabs General / Models, "Save model provider", "Model provider saved" status
  - General, Models with many models, and model errors, at 1440 and 390 px
  - try the Models tab with one line per model (ID, wire API, limits, remove) on the wide page

  Settle the layout with the user in the design session, export the boards and get approval. Update the `model-providers` delta if the approved layout changes it.
- [ ] 1.2 PenPot (page "Tasks - Details Sidebar"): the "Description" header with the chevron, collapsed and expanded. Export and get approval.
- [ ] 1.3 PenPot (transcript board): a user message with a highlighted `rust` fence and a multi-line paragraph, light and dark. Export and get approval.
- [ ] 1.4 `docs/design-workflow.md`, "Mapping views to boards":
  - rename the "Model providers · edit, …" rows to "settings page" rows with route `/model-providers/<id>` (and `?tab=models`)
  - add rows for the description chevron and the user message board
  - verify that every board name matches PenPot

## 2. Shared settings page frame (design D2)

- [ ] 2.1 Extract the frame of `config_pages/project_page.rs` into a helper in `config_pages.rs`: back link, header row with heading markup, `#save-status`, save button, form. The project page uses it, and `config_pages/tests/project_page.rs` stays green unchanged.

## 3. Model provider settings page (design D3, D4)

- [ ] 3.1 Write failing tests in `config_pages/providers/tests.rs`:
  - `GET /model-providers/{id}` shows the sidebar (Model providers current), the back link, the header name, the General / Models tabs and "Save model provider"
  - `?tab=models` opens Models, and an unknown value opens General
  - an unknown id and `/model-providers/{id}/edit` answer 404
  - the list row's name and edit action link to the page
  - a save re-renders the form on the posted tab and says "Model provider saved" in a polite status region, without `modal-close`
  - a validation error opens the tab of the first error and keeps the values
  - creating a provider answers `HX-Location` to its page
  - refreshing in edit mode keeps the page tab style and the current tab

  Adapt the existing tests that open the edit modal.
- [ ] 3.2 Add `Nav::ModelProviderSettings` in `layout.rs` (Model providers section, no bottom bar or floating button), with a test next to the `ProjectSettings` layout tests.
- [ ] 3.3 Implement `config_pages/providers/page.rs` and the route changes in `providers.rs`:
  - page handler
  - `TabsStyle` by mode
  - `update` saved response with out-of-band heading and status
  - `create` with `HX-Location`
  - list row links
  - `/edit` route and `edit` handler removed

  Lay out the Models tab as approved in 1.1.
- [ ] 3.4 `scripts/tabs.ts`: `init()` keeps the tab the server opened and writes it to the URL, instead of opening the tab from `?tab=` (design D3). Update the header comment.
- [ ] 3.5 E2E in `crates/df-e2e/tests/ui.rs`:
  - open a provider from the list, choose Models (URL gets `?tab=models`), add a model, save, and check that the page stays open with "Model provider saved"
  - on `?tab=models`, save with an `http://` upstream URL: General opens with the error, and the URL loses `?tab`
  - the same check on the project settings page: on `?tab=push-checks`, save with an `ssh://` repository URL

  Write these first; the error cases fail before 3.4. Run `just e2e-local` if a cluster and container socket are available; otherwise state that CI runs it.

## 4. Description chevron (design D5)

- [ ] 4.1 Write a failing test in `task_pages/tests/detail.rs`: the description `details` has the `group` class, a `list-none` summary and both chevrons (right while closed, down while open). Then implement it in `task_pages/view.rs`.

## 5. Markdown in user messages (design D6)

- [ ] 5.1 Write failing tests in `markdown.rs`: `render_with_hard_breaks` turns a single newline into `<br>`, renders `a\n\n\n\nb` as two paragraphs, and escapes raw HTML, drops `javascript:` links and turns images into links, like `render`. Implement it with options shared with `render`.
- [ ] 5.2 Write failing tests in `components/transcript.rs` and `transcript/tests.rs`:
  - a user message with a `rust` fence contains `hl-` spans inside `md-body`
  - `<b>x</b>` stays escaped
  - hidden parts stay plain text
  - the queued stack stays plain

  Adapt `messages_escape_their_text_and_keep_their_ids` and the tests that expect `whitespace-pre-wrap`. Then change `user_message`.
- [ ] 5.3 Check `.md-body` on `bg-bubble-me` in light and dark against the board of 1.3 (inline code, links, code block, first and last child margins) and adjust `styles/input.css` where needed.

## 6. Verification

- [ ] 6.1 `cargo test -p df-web`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length`, and `cargo machete`.
- [ ] 6.2 Check the provider settings page, the description header and a user message with code at 1440 and 390 px against the approved boards (`just serve-local`, if a container socket is available). Attach screenshots to the review, or state that this check was skipped.

## 7. Docs

- [ ] 7.1 `docs/content/using/model-providers.md`:
  - a provider is edited on its settings page (`/model-providers/<id>`, `?tab=models` links to the Models tab)
  - "New model provider" opens the new page after creation
  - delete stays in the list
- [ ] 7.2 `docs/content/using/tasks.md`:
  - user messages are rendered as Markdown with highlighted code and kept line breaks
  - hidden blocks and the queued stack stay plain
  - the Description section is collapsible
- [ ] 7.3 Run `just docs-build`.
