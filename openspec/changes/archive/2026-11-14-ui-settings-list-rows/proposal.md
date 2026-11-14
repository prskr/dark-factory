# Proposal

## Why

The settings lists for projects, model providers and credential references are one-line tables. At about 1350 px browser width, the project table no longer fits its card. Its edit button sits at the end of the row, so the user has to scroll the table sideways to reach it. The model provider table wraps its "Wire APIs" cell, so one row ends up taller than the others.

## What Changes

- **BREAKING (UI):** from 768 px, the lists of projects, model providers and credential references show **two-line rows**:
  - Line 1 is the entry's name, which is the edit link.
  - Line 2 holds the details in smaller muted text:
    - projects: forge icon, repository as host and path (no scheme, no `.git`), branch icon and base branch
    - model providers: wire APIs · scheme · credential
    - credential references: secret / key
  - The project key sits in its own column to the left and spans both lines.
  - Counts, "Used by" and delete sit on the right and are centered across both lines.
  - Headers show one label on the left ("Project", "Name"), plus labels only for the columns on the right.
- **BREAKING (UI):** **the whole row opens the entry.**
  - The row opens what its name opens: the project settings page, the model provider settings page (`ui-model-provider-page`), or the credential reference modal.
  - The pencil edit buttons are removed from these three lists.
  - Hovering a row shades it and underlines the name.
  - When the row's link has keyboard focus, the row shows a focus ring.
  - Count links and the delete button keep working on their own.
- **Zero counts hidden:** in the desktop project list, a count of 0 shows no number and is no link. Screen readers still hear "Queued: 0 tasks of DF". Non-zero counts keep their links.
- The approved PenPot boards for these lists are:
  - "Settings · projects, two-line rows (1350)" (Config - Projects)
  - "Model providers · two-line rows (1350)" (Config - Model providers)
  - "Credentials · two-line rows (1350)" (Config - Credentials)

  They replace "Settings · projects", "Model providers · table" and "Credentials · table" as the reference for these lists.

Sequencing: this change builds on `ui-model-provider-page`, which moves provider editing to `/model-providers/{id}`. That change is implemented first. The model provider row then links to the settings page.

Out of scope:
- **Phone project list (below 768 px):** it keeps its card layout, its edit icon and its muted zero counts. It already fits 390 px. Rejected.
- **Egress rules table:** it has no edit action, so there is nothing to open with a row click. Rejected.
- **Copy button for the repository URL in the row:** the URL is under the row's click layer, so it can't be selected. It can be copied from the project settings page. Rejected.
- **Persona list:** it uses cards, not a table. Rejected.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `configuration-management`:
  - New requirement "Settings list rows": the two-line layout, the row as the edit target, hover and focus states, and actions that stay separate.
  - "Project overview with task counts": zero counts are hidden in the desktop list and are no links.

## Impact

- Code (`df-web`):
  - `components/table.rs`: a two-line row, a row link that covers the row, and actions lifted above it. The `Cell::Actions` doc comment changes.
  - `config_pages.rs`: `projects_region` (desktop table: key column, two-line project cell, zero counts), `credentials_region`, `count_link`.
  - `config_pages/providers.rs`: `region` and `row` (on top of `ui-model-provider-page`).
- `edit_button` stays: personas and the phone project list still use it.
- Tests:
  - `components/table.rs`
  - `config_pages/tests/mod.rs`: project list, credentials list, phone list unchanged
  - `config_pages/tests/project_page.rs`: the list's edit action
  - `config_pages/providers/tests.rs`
  - `crates/df-e2e/tests/ui.rs`: the helper that opens a project through `Edit project {key}`
- Design: PenPot boards as above. `docs/design-workflow.md` maps the rows of the three list boards.
- Docs: `docs/content/using/setup.md` (how a project is opened from the list) and `docs/content/using/model-providers.md` (how a provider is opened and deleted).
- No database, API, configuration or Helm change.
