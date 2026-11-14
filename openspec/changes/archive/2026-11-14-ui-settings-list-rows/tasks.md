# Tasks

## 0. Precondition

- [x] 0.1 Check that `ui-model-provider-page` is implemented: `GET /model-providers/{id}` renders the settings page, and `providers.rs::row` links the name to it. If not, stop apply and report it.

## 1. Design (PenPot)

- [x] 1.1 PenPot: the boards "Settings · projects, two-line rows (1350)", "Model providers · two-line rows (1350)" and "Credentials · two-line rows (1350)" are drawn and approved in the design session of 2026-11-11.

## 2. Shared row pattern (design D1–D3)

- [x] 2.1 Write failing tests in `components/table.rs`:
  - the two-line name cell is `w-full max-w-0` (design D1) and renders a truncating name line and a muted, truncating details line
  - the row is `relative group` with `hover:bg-sunk`
  - the page form of the entry control is an `<a href>` that covers the row (`after:absolute after:inset-0`) with the ring classes of D3
  - the modal form is a `<button>` with `hx-get`, `hx-target="#composer-slot"` and the given `aria-label`
  - the action cell content is `relative z-10`
- [x] 2.2 Implement the helpers in `components/table.rs` and update the module and `Cell::Actions` doc comments (board names, no edit button). Verify that `cargo test -p df-web components::table` passes.
- [x] 2.3 Add `Icon::GitBranch` (Lucide `git-branch`) to `components/icons.rs`. Verify with the icon test. (Not needed: the existing `Icon::Git` is already the Lucide `git-branch` glyph and the branch icon elsewhere; design D5 allows it. The list uses `Icon::Git`.)

## 3. Projects list (design D1, D4, D5)

- [x] 3.1 Write failing tests in `config_pages/tests/mod.rs` for the desktop table:
  - a screen-reader-only "Key" header, then "Project" and the four count headers, and no Repository, Branch or Actions header
  - the row has the key chip (`group-hover:bg-panel`), the name link to `/projects/DF` covering the row, and the details line with forge icon, repository URL, branch icon and `main`
  - no `aria-label="Edit project DF"` in the desktop table
  - a zero count renders only `sr-only` "Running: 0 tasks of WEB", with no `href`; a non-zero count stays a `relative z-10` link
- [x] 3.2 Keep the phone list test (`Edit project DF` in the phone card, muted "0 running") green unchanged.
- [x] 3.3 Implement `projects_region` (desktop part) and `count_link` (zero on desktop). Verify the tests from 3.1 and 3.2 with `cargo test -p df-web config_pages`.
- [x] 3.4 Update `config_pages/tests/project_page.rs`: the list's entry link opens `/projects/DF`, where the test looked for the edit button before. Verify with `cargo test -p df-web project_page`.

## 4. Credential references list (design D2, D5)

- [x] 4.1 Write failing tests in `config_pages/tests/mod.rs`:
  - header "Name" only
  - the row has a `<button>` named `anthropic-api` with `aria-label="Edit credential reference anthropic-api"` and `hx-get="/credentials/{id}/edit"`, covering the row
  - the details line reads `factory-anthropic / api-key`
  - there is no pencil button
- [x] 4.2 Implement `credentials_region`. Verify the tests from 4.1. Verify that the existing modal-edit tests (save closes the modal, list updated) still pass.

## 5. Model providers list (design D2, D5)

- [x] 5.1 Write failing tests in `config_pages/providers/tests.rs`:
  - headers "Name" and "Used by", plus an actions column without a visible label
  - the name link to `/model-providers/{id}` covers the row
  - the details line holds wire APIs, scheme and credential, with `aria-hidden` separators
  - "missing" appears for a deleted credential
  - the delete button is `relative z-10` with `aria-label="Delete model provider OpenAI"` and the confirmation text unchanged
  - there is no edit button
- [x] 5.2 Implement `region` and `row` in `config_pages/providers.rs`. Verify the tests from 5.1 and the existing delete tests.

## 6. End-to-end and docs

- [x] 6.1 `crates/df-e2e/tests/ui.rs`:
  - `open_project_page` opened a project through `[aria-label='Edit project {key}']`. It is also called at 390 and 300 px. It now clicks the visible name link `a[href='/projects/{key}']`, which exists both in the desktop row and in the phone card. Verify that the desktop tests and the phone tab test (390 and 300 px) still pass.
  - new test at 1350 px viewport, with one project whose repository URL is longer than 200 characters:
    - clicking the repository URL of a project row opens `/projects/{key}`
    - clicking a Running count opens the filtered task list
    - the project table has no horizontal scrollbar (`scrollWidth <= clientWidth` of the card), and the Done count of the long-URL row lies inside the viewport

  Verify with `just e2e-local`. If no container socket is available, state that in the report.
- [x] 6.2 Docs:
  - `docs/content/using/setup.md`: a project is opened by its row in the list (desktop) or its edit icon (phone); empty counts are left blank
  - `docs/content/using/model-providers.md`: a provider is opened by its row; delete stays in the row
  - the credential references section (setup.md, if it describes editing)

  Verify with `just docs-build`.

## 7. Design check and cleanup

- [x] 7.1 Compare `/projects`, `/model-providers` and `/credentials` at 1350 px with the three boards, following `docs/design-workflow.md` (PenPot + Playwright MCP). Fix the deviations, or record them in the mapping notes.
- [x] 7.2 PenPot: delete the superseded boards "Settings · projects", "Model providers · table" and "Credentials · table", after the user confirms. Update the mapping rows in `docs/design-workflow.md` to the new board names, viewport 1350, and verify that every name matches PenPot.
- [x] 7.3 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length` and `cargo machete`. Verify that all are clean.
