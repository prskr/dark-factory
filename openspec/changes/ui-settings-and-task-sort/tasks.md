# Tasks

Prerequisite: `generate-webhook-secrets` is implemented and archived (webhook secret source).

## 1. Design (PenPot, design D7)

- [x] 1.1 PenPot (page "Config - Personas"): add the Projects state to the variant set `Settings nav` as the first entry. On page "Config - Projects", put the projects table board inside the settings area (rail without Projects, Settings marked current). Export the boards and get the user's approval.
- [x] 1.2 PenPot (page "Config - Projects"): turn the edit modal into the project settings page (design D8). It shows the settings sidebar, a back link "Projects", a header with name, key and forge icon, the tabs, "Save project" and the "Project saved" status. Draw it at 1440 and 390 px in two variants, vertical and horizontal tabs. Settle the orientation with the user in the design session. Export the boards and get the user's approval.
- [x] 1.3 PenPot (task list page): add the sort menu to the list title row (closed, and open with "Recently updated" checked) at 1440 and 390 px. Export the boards and get the user's approval.
- [x] 1.4 PenPot (page "Config - Projects"): on the approved page layout of 1.2, turn the "edit modal, lifecycle events" boards into an "Integrations" tab with two groups (heading and separator):
  - "Lifecycle events" with the current content
  - "Incoming webhook" with the URL, a copy button, the secret masked, revealed and just copied (check icon, "Copied")
  - the unavailable states: no forge integration, no webhook support, no `publicUrl`

  Export the boards and get the user's approval.
- [x] 1.5 PenPot (page "Config - Projects"): group the General tab and the new-project form. Start from Project / Repository / Sandbox / Forge events, and try a variant with the wake policy under Integrations › Incoming webhook. Settle the grouping with the user, then update the `Grouped project form` requirement in `specs/configuration-management/spec.md` (and, if the wake policy moves, the `forge-webhooks` delta and design D7) to the approved groups.
- [x] 1.6 Add the new and renamed boards to "Mapping views to boards" in `docs/design-workflow.md`: rename the "edit modal" rows to "settings page" with route `/projects/<key>` (and `?tab=…`), rename the "lifecycle events" rows to "integrations", and give routes, data and viewports. Verify that every board name matches PenPot.

## 2. Top-level `publicUrl` (design D4)

- [x] 2.1 Write failing config tests in `crates/df-core/src/config.rs`:
  - top-level `publicUrl` parses
  - `auth.publicUrl` is rejected as an unknown field
  - `auth` without `publicUrl` fails validation with a message naming `publicUrl`
  - `http://` gives the warning flag

  Then move the field and its validation. Verify `cargo test -p df-core config` passes.
- [x] 2.2 Update the OIDC code (`crates/df-web/src/auth/oidc.rs`) and `dark-factory` wiring to read the top-level value. Add `public_url: Option<Url>` to `WebState` and the testkit. Add a response-header test: after a login with a non-loopback `http://` `publicUrl`, the `Set-Cookie` of the session has `HttpOnly` and `SameSite=Lax` and no `Secure` (the existing HTTPS test keeps `Secure`). Verify `cargo test -p df-web auth` passes.
- [x] 2.3 Write failing Helm unit tests in `deploy/helm/dark-factory/tests/certificates_and_auth_test.yaml`:
  - `publicUrl` set → instance config has top-level `publicUrl` and an `auth` section
  - `publicUrl` empty → no `auth` section
  - `oidc.publicUrl` → schema error

  Then change `values.yaml`, `values.schema.json` and `templates/configmap.yaml`, and update `deploy/values-dev.yaml`, `deploy/values-e2e.yaml` and the README rows. Verify `build/with_tools.sh helm-unittest deploy/helm/dark-factory` passes.

## 3. Projects in the settings area and project settings page (design D1, D8)

- [x] 3.1 Write failing tests in `crates/df-web/src/components/shell.rs` and `layout.rs`:
  - the rail lists Tasks, Requests, Network traffic and Settings, and no Projects
  - `/projects` renders the settings sidebar with Projects first and current, and the rail marks Settings current
  - the Settings link points to `/projects`
  - the phone bottom bar still lists Tasks, Requests and Projects, and the floating button reads "New project" on `/projects`

  Then change `SECTIONS`, `SETTINGS_SECTIONS`, `SETTINGS_HOME` and `is_settings`. Verify `cargo test -p df-web shell` and `cargo test -p df-web layout` pass.
- [x] 3.2 Project settings page (design D8). Write failing tests in `crates/df-web/src/config_pages/tests/mod.rs` (a new `project_page.rs` if it grows):
  - `GET /projects/DF` renders a full page with the settings sidebar (Projects current), back link, header (name, key, forge icon), the tabs and "Save project"
  - `?tab=push-checks` and `?tab=integrations` (admin) open that tab; an unknown value or `integrations` for a `board-member` opens General
  - a save answers with the re-rendered page content and a "Project saved" status, not `modal-close`
  - a validation error opens the tab with the first error and keeps the values
  - unknown key → `404`; `/projects/DF/edit` no longer exists (`404`)
  - the project list's name and edit action link to `/projects/DF`
  - creating a project answers with `HX-Location` to the new page

  Then:
  - replace `project_edit` with the page handler
  - render `tabbed_form` on the page in the orientation approved in task 1.2
  - make `scripts/tabs.ts` read and write `?tab=` (`history.replaceState`)
  - rename the panel id `lifecycle-events` to `integrations`

  Verify `cargo test -p df-web config_pages` passes.

## 4. Task list sort (design D2, D3)

- [x] 4.1 Add `TaskSort` to `df-core` (`domain.rs`) with URL parsing (`updated`, `updated-asc`, `created`; default for unknown values). Write failing `df-db` tests for `list_task_items` in each sort (the updated sorts by `updated_at`), including a stable tie order. Then change the query and regenerate `.sqlx/`. Verify `cargo test -p df-core` and `cargo test -p df-db task_views` pass (the latter container-backed; state if no socket is available and CI runs them).
- [x] 4.2 Write failing tests in `crates/df-web/src/task_pages/tests/list.rs`:
  - the default list is ordered by `updated_at`, newest first
  - `sort=created` orders by creation and appears in chip, project and row links
  - an unknown `sort` falls back to the default
  - the sort menu marks the current option and names it on the button
  - the SSE URL carries the sort

  Rename `task_list_shows_newest_first_with_status_and_status_line` to match the new behavior. Then extend `Filters`, add the sort menu to `list_column` (Lucide `arrow-down-up` added to `Icon`) and pass the sort through `sse.rs`. Verify `cargo test -p df-web task_pages` passes.
- [x] 4.3 Write a failing `sse.rs` test: a `TaskChanged` from a status change re-renders the list in the new order. Verify `cargo test -p df-web sse` passes.

## 5. Grouped project form (design D6)

- [x] 5.1 Add `form_group(title, body)` to `components/forms.rs`, with a test for the region, heading and separator (none above the first group).
- [x] 5.2 Write failing tests in `crates/df-web/src/config_pages/tests/mod.rs`:
  - the settings page and the new-project form show the approved groups in order
  - a validation error renders inside its group with the values kept
  - field names are unchanged

  Then restructure `project_form`. Verify `cargo test -p df-web config_pages` passes.

## 6. Integrations tab (design D5, D6)

- [x] 6.1 Write failing tests in `crates/df-web/src/config_pages/tests/lifecycle_tab.rs` (rename the file to `integrations_tab.rs`) and `project_subscriptions/tests.rs`:
  - the admin tab is called "Integrations" and comes last
  - it shows the "Lifecycle events" group (public key, subscriptions, off state) followed by the "Incoming webhook" group
  - non-admins see no tab
  - the subscription fragment routes still answer `403` without admin; remove the plain-browser redirect and its tests
  - leaving the page after creating a subscription keeps it (subscriptions save on their own)

  Then rename the tab and wrap the subscription region in `form_group`. Verify `cargo test -p df-web` passes.
- [x] 6.2 Write failing tests for the "Incoming webhook" group:
  - full URL from `publicUrl`
  - path and hint without it
  - each unavailable state (no plugin, no forge credential, not loaded, no `webhooks` export, no secret)
  - the masked placeholder, with the secret absent from the response body
  - accessible names of the eye and clipboard buttons
  - the "Copying needs HTTPS" note is in the markup, hidden unless the client finds no secure clipboard

  Then implement the group, using the same serving-plugin resolution as the project list. Verify `cargo test -p df-web` passes.
- [x] 6.3 Write failing tests for `GET /projects/{key}/webhook-secret`:
  - `admin` gets the secret as `text/plain` with `Cache-Control: no-store`, and an audit entry `webhook_secret.read` (actor, project, plugin, no secret)
  - `board-member` gets `403` with no audit entry
  - unavailable states get `404`
  - unknown project gets `404`

  Then add the audit action to `df-core` and the route behind `require_admin`, and expose the per-plugin secret to `WebState`. Verify `cargo test -p df-web` passes. Database-backed audit tests are container-backed; state if no socket is available.
- [x] 6.4 Add Lucide `eye`, `eye-off` and `copy` to `Icon`. Add `scripts/secret-field.ts` (Alpine component):
  - fetch on show or copy
  - hide drops the value from the DOM
  - `navigator.clipboard.writeText`
  - without `window.isSecureContext` or `navigator.clipboard`: hide both copy buttons and show "Copying needs HTTPS" (design D5)
  - check icon and polite "Copied" for 2 s
  - inline error on failure

  Register it like the other scripts. Verify the compiled-asset tests run with `SWC_BIN` available, or state that they were reported as ignored.

## 7. End-to-end and visual checks

- [x] 7.1 Extend `crates/df-e2e/tests/ui.rs`:
  - Projects is reached through Settings at 1440 px and through the bottom bar at 390 px
  - `open_project_dialog` becomes opening the project settings page; saving stays on the page and shows "Project saved"; `?tab=integrations` opens that tab
  - the phone tab-row test chooses "Integrations" on the page

  Update `crates/df-e2e/tests/lifecycle_events.rs` to reach the subscriptions through the page.
  - the task list sort menu changes the order and the URL

  Extend `crates/df-e2e/tests/webhooks.rs`: the URL shown in the Integrations tab is the one deliveries succeed on, and "Copy secret" puts the secret on the clipboard (clipboard permission granted in the browser context). Run `just e2e-local` if a cluster and container socket are available; otherwise state that CI runs them.
- [x] 7.2 Check the settings area with Projects, the project settings page, the task list with the sort menu, the Integrations tab and the grouped General tab at 1440 and 390 px against the approved PenPot boards (`just serve-local`, if a container socket is available). Verify with screenshots attached to the review, or state that this check was skipped.

## 8. Docs

- [x] 8.1 `docs/content/using/setup.md`:
  - Projects is reached through Settings on desktop and the bottom bar on phones
  - a project is edited on its settings page (`/projects/<key>`, tabs linkable with `?tab=`); "New project" opens the new page after creation
  - the General field table follows the new groups
  - the admin tab is "Integrations", with both groups described

  `docs/content/using/tasks.md`:
  - sort options and default, and that only status changes and renames count as an update
  - fix the Done group to include `cancelled`

  Verify with `just docs-build`.
- [x] 8.2 `docs/content/integrations/forges/webhooks.md`:
  - "Register the webhook" takes the URL and secret from the project's settings page, tab Integrations → Incoming webhook (admin)
  - keep the `kubectl` way as an alternative
  - reading the secret is audited
  - the copy buttons need HTTPS (or a loopback host); over plain HTTP use "Show secret" and copy by hand
  - "Choose who wakes tasks": replace "Projects → edit a project → General" with the settings page path (and the new tab if the wake policy moved)

  `docs/content/integrations/lifecycle-events.md` and `docs/content/authentication/_index.md` and `keycloak.md`: replace "edit a project → tab Lifecycle events" with the settings page, tab "Integrations" › "Lifecycle events". Verify with `just docs-build`.
- [x] 8.3 Configuration reference and upgrade note for `publicUrl` (top level; required with `auth`; no default; the effect on OIDC and the webhook URL; the old keys are rejected) in:
  - `docs/content/authentication/_index.md`, `dex.md`, `keycloak.md`, `authentik.md`
  - `docs/content/getting-started.md`
  - `docs/content/operations/multiple-instances.md`
  - `deploy/helm/dark-factory/README.md`

  Verify with `just docs-build`.

## 9. Third-party scripts from the npm lockfile (design D9)

- [x] 9.1 Find the published versions of today's vendored files. Fetch candidate tarballs and compare SHA-256:
  - `mermaid` 11.17.2 (`dist/mermaid.min.js`, hash in `crates/df-web/README.md`)
  - `htmx.org` 4.x (`dist/htmx.min.js`, `dist/ext/hx-sse.min.js` or `dist/ext/hx-sse.js`)
  - `alpinejs` 3.x (`dist/cdn.min.js`)

  Record the matching versions in the review. If a file matches no published version, name the closest and the difference, and ask the user before pinning it.
- [x] 9.2 Add `crates/df-web/package.json` (`private`, `packageManager` as in `deploy/images/claude-harness/package.json`, the three packages at the versions from 9.1) and `pnpm-workspace.yaml` (`allowBuilds: {}`). Generate `pnpm-lock.yaml` with `pnpm install --lockfile-only`, and add `crates/df-web/node_modules/` to `.gitignore`. Verify that `pnpm install --frozen-lockfile` in `crates/df-web` produces the four files with the same bytes as the checked-in ones.
- [x] 9.3 Write failing tests in `crates/df-web/build_assets.rs`:
  - the `VENDOR` table maps every served path to a package file
  - `VENDOR_SCRIPTS` files are staged under their served paths
  - without `VENDOR_SCRIPTS`, the `node_modules` files are used
  - one file missing → none staged, a warning naming the missing ones and the install command, cfg `df_web_vendor` not set
  - missing with `DF_WEB_REQUIRE_ASSETS=1` → error

  Then add the `VENDOR` kind to `stage_assets`, plus the `rerun-if-*` lines and the `rustc-check-cfg` in `build.rs`. Verify `cargo test -p df-web build_assets` passes (if the `build_assets.rs` tests run through a separate target, use that).
- [x] 9.4 Bazel:
  - add `npm.npm_translate_lock(name = "npm_df_web", pnpm_lock = "//crates/df-web:pnpm-lock.yaml")` and `use_repo` to `MODULE.bazel`
  - in `crates/df-web/BUILD.bazel`, add `npm_link_all_packages` and a `vendor_scripts` target that selects the table's files from `:node_modules/mermaid`, `:node_modules/htmx.org` and `:node_modules/alpinejs`
  - pass it to `:build_script` as `VENDOR_SCRIPTS` and in `data`
  - run `bazel mod tidy` and commit `MODULE.bazel.lock`

  Verify `bazel --bazelrc=.bazelrc.agents build //crates/df-web` and `bazel --bazelrc=.bazelrc.agents test //crates/df-web/...` pass.
- [x] 9.5 Delete `crates/df-web/assets/vendor/` (all four files). Update the tests in `crates/df-web/src/assets.rs`:
  - `embeds_stylesheet_and_vendored_scripts` keeps only the stylesheet
  - `vendored_scripts_are_embedded_byte_for_byte` compares each served path with its package file
  - a version test compares the installed packages with `crates/df-web/package.json`
  - the htmx cache-header test

  Give all of them `#[cfg_attr(not(df_web_vendor), ignore = "needs third-party scripts: pnpm install --frozen-lockfile in crates/df-web, or VENDOR_SCRIPTS")]`. Verify `cargo test -p df-web assets` passes with and without `node_modules`; without it those tests are listed as ignored.
- [x] 9.6 Write failing tests: the layout renders `data-mermaid-src` only when the asset is embedded (`crates/df-web/src/layout.rs` tests, cfg-gated both ways), and `scripts/markdown.ts` shows "Diagram renderer not available" when `data-mermaid-src` is missing or the script fails to load (TypeScript source test as for other components, plus the e2e check in 7.1 if a browser is available). Then implement. Verify `cargo test -p df-web layout` and `cargo test -p df-web markdown` pass.
- [x] 9.7 Check that Renovate picks up `crates/df-web/package.json`: no `ignorePaths` or `enabledManagers` excludes it in `renovate.json`, and the Renovate workflow's pnpm install covers a second lockfile. htmx and its extension are one package, so they need no grouping rule. Add a `packageRules` entry only if an update needs a schedule or manual approval (for example htmx majors). Verify with `npx --yes --package renovate@<RENOVATE_VERSION> renovate-config-validator` if available, or state that the dry run was skipped.
- [x] 9.8 Docs (contributor docs; no published page describes how the scripts are vendored, so `docs/content/` is unchanged):
  - root `AGENTS.md` ("Plain `cargo` builds"): the third-party scripts join the compiled-asset exception
  - `crates/df-web/AGENTS.md`: no third-party script is checked in, and the install step
  - `crates/df-web/README.md`: replace "vendored in" (intro) and the "Vendored Mermaid" note with `package.json`, the served-path table and the plain-`cargo` install step
  - `docs/development.md`: `pnpm install --frozen-lockfile` in `crates/df-web` for a working UI under plain `cargo`; the Renovate section names the new lockfile
  - `docs/adr/0047-…`: a note that "vendored" now means "pinned in the npm lockfile"

  Verify that the links resolve (`just docs-build` for the docs site).

## 10. Integration checks

- [x] 10.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length`, `cargo xtask check-test-names` and `bazel --bazelrc=.bazelrc.agents test //...`. State which container-backed tests could not run. Verify all pass.
- [x] 10.2 Run `openspec validate ui-settings-and-task-sort --strict`; verify it passes.

## Workflow follow-up

- Before archiving, add a backlog row in `product/iterations/roadmap.md` for "Rotate or regenerate the webhook secret from the Integrations tab" (reference: this change).