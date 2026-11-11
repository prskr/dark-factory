## 1. Design (PenPot, design D1)

- [ ] 1.1 PenPot (page "Config - Model providers"): turn the "Model providers · edit, …" boards into the model provider settings page:
  - settings sidebar with Model providers current, back link "Model providers", header with the name, horizontal tabs General / Models, "Save model provider", "Model provider saved" status
  - General, Models with many models, and model errors, at 1440 px
  - try the Models tab with one line per model (ID, wire API, limits, remove) on the wide page

  Settle the layout with the user in the design session, export the boards and get approval. Update the `model-providers` delta if the approved layout changes it.
- [ ] 1.2 PenPot (page "Tasks - Details Sidebar"): the "Description" header with the chevron, collapsed and expanded, and the project and persona values as links. Export and get approval.
- [ ] 1.3 PenPot (transcript board): a user message with a highlighted `rust` fence and a multi-line paragraph, light and dark. Export and get approval.
- [ ] 1.4 `docs/design-workflow.md`, "Mapping views to boards":
  - rename the "Model providers · edit, …" rows to "settings page" rows with route `/model-providers/<id>` (and `?tab=models`)
  - add rows for the description chevron and the user message board
  - "Config - Projects": rename the "push checks" rows to "security" with route `/projects/<key>?tab=security`, and name the tab "Security" in the page summary and the note on unbuilt tabs
  - verify that every board name matches PenPot
- [ ] 1.5 PenPot (page "Config - Projects"): the tab label "Security" instead of "Push checks" on every project settings board, and rename the boards "Projects · settings page, push checks" and "…, push checks, invalid glob" to "…, security" (design D7). Export and get approval.

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
- [ ] 3.4 `scripts/tabs.ts`: `init()` keeps the tab the server opened and writes it to the URL, instead of opening the tab from `?tab=` (design D3). Update the header comment. This fixes TD-124: the project settings e2e of 3.5 is its failing test. Delete the TD-124 row and section from `docs/tech_debt.md`.
- [ ] 3.5 E2E in `crates/df-e2e/tests/ui.rs`:
  - open a provider from the list, choose Models (URL gets `?tab=models`), add a model, save, and check that the page stays open with "Model provider saved"
  - on `?tab=models`, save with an `http://` upstream URL: General opens with the error, and the URL loses `?tab`
  - the same check on the project settings page: on `?tab=security`, save with an `ssh://` repository URL
  - the existing project settings test expects `?tab=security` after choosing Security

  Write these first; the error cases fail before 3.4. Run `just e2e-local` if a cluster and container socket are available; otherwise state that CI runs it.

## 4. Description chevron (design D5)

- [ ] 4.1 Write a failing test in `task_pages/tests/detail.rs`: the description `details` has the `group` class, a `list-none` summary and both chevrons (right while closed, down while open). Then implement it in `task_pages/view.rs`.

- [ ] 4.2 Write a failing test in `task_pages/tests/detail.rs`: the details region links the project value to `/projects/DF` and the persona value to `/personas`. Then implement it in `details_region` (design D5a).

## 5. Markdown in user messages (design D6)

- [ ] 5.1 Write failing tests in `markdown.rs`: `render_with_hard_breaks` turns a single newline into `<br>`, renders `a\n\n\n\nb` as two paragraphs, and escapes raw HTML, drops `javascript:` links and turns images into links, like `render`. Implement it with options shared with `render`.
- [ ] 5.2 Write failing tests in `components/transcript.rs` and `transcript/tests.rs`:
  - a user message with a `rust` fence contains `hl-` spans inside `md-body`
  - `<b>x</b>` stays escaped
  - hidden parts stay plain text
  - the queued stack stays plain

  Adapt `messages_escape_their_text_and_keep_their_ids` and the tests that expect `whitespace-pre-wrap`. Then change `user_message`.
- [ ] 5.3 Check `.md-body` on `bg-bubble-me` in light and dark against the board of 1.3 (inline code, links, code block, first and last child margins) and adjust `styles/input.css` where needed.

## 6. Security tab (design D7)

- [ ] 6.1 Adapt the tests first: `config_pages/tests/project_page.rs`, `config_pages/tests/integrations_tab.rs` and `config_pages/push_policy/tests.rs` expect the label "Security" and the tab id `security`. Add a test: `?tab=push-checks` opens General. Then change the tab id and label in `config_pages.rs` and the doc comments that name the tab in `config_pages.rs`, `project_page.rs` and `push_policy.rs`.

- [ ] 6.2 Adapter command with shell quoting (design D8, TD-60):
  - Write failing tests in the project form tests of `config_pages/tests/`:
    - `/opt/adapter '--config=/workspace/my config.json'` is stored as two arguments
    - the settings page shows the stored command quoted, and saving it unchanged stores the same arguments
    - an unclosed quote answers 422 with the error at Adapter command, and nothing is stored
    - an empty field stores no arguments
    - `'--empty' ''` keeps the empty argument, and `"say \"hi\""` stores `say "hi"`
    - pin the documented edge cases: an unquoted `#fff` is dropped as a comment while `'#fff'` is kept, and `\\` stores one backslash
  - Add `shlex` to `[workspace.dependencies]` and `df-web` (skill `rust`, `cargo deny check`).
  - Use `shlex::split` in `validate_project` and `shlex::try_join` in `project_values`, and mention quoting in the field hint.
  - Delete the TD-60 row and section from `docs/tech_debt.md`.

## 7. Verification

- [ ] 7.1 `cargo test -p df-web`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length`, and `cargo machete`.
- [ ] 7.2 Check the provider settings page at 1440 px, and the description header, a user message with code and the project settings Security tab at 1440 and 390 px, against the approved boards (`just serve-local`, if a container socket is available). Attach screenshots to the review, or state that this check was skipped.

## 8. Docs

- [ ] 8.1 `docs/content/using/model-providers.md`:
  - a provider is edited on its settings page (`/model-providers/<id>`, `?tab=models` links to the Models tab)
  - "New model provider" opens the new page after creation
  - delete stays in the list
- [ ] 8.2 `docs/content/using/tasks.md`:
  - user messages are rendered as Markdown with highlighted code and kept line breaks
  - hidden blocks and the queued stack stay plain
  - the Description section is collapsible
  - the project and persona in the task details link to the project settings and the persona list
- [ ] 8.3 The tab is named **Security** and the URL uses `?tab=security` in `docs/content/using/setup.md`, `docs/content/using/hidden-paths.md` and `docs/content/security/git-proxy.md`. The section heading "Push checks" in `git-proxy.md` stays, because it describes the checks, not the tab. In `setup.md`, the Adapter command row says that arguments use shell-style quoting, with an example of an argument containing a space. It also says that nothing is expanded (`$HOME` stays literal), that a word starting with `#` must be quoted (`'#fff'`, otherwise it starts a comment and is dropped), and that a literal backslash is written `\\`.
- [ ] 8.4 Run `just docs-build`.
