# Tasks

Prerequisite: the change `lifecycle-events` is archived (its manual tasks 5.3 and 5.4 are done), so that `openspec/specs/event-subscriptions/spec.md` exists for this delta.

## 1. Tab and fragment endpoints

- [x] 1.1 Tests first (`project_subscriptions/tests.rs`): `GET /projects/{key}/subscriptions` with `HX-Request` returns the tab fragment (key panel, list, "New subscription") and without it redirects to `/projects`; board member gets 403 with and without `HX-Request` (no redirect); feature-off and unrestricted variants render their notes. Implement the fragment (`region` without page header) and the redirect; verify with `cargo test -p df-web project_subscriptions`
- [x] 1.2 Tests first (`config_pages` tests): the edit form of an admin has the tabs "General", "Push checks", "Lifecycle events" in this order with a lazily loaded panel (design D1); a board member's edit form has no such tab; the project list has no lifecycle events row action. Implement in `project_form` / `project_edit` and remove `settings_link`; verify with `cargo test -p df-web config_pages`

## 2. Inline form, save and delete

- [x] 2.1 Tests first: `new` and `edit` return the editor fragment with inputs bound to `form="subscription-form"`; the admin's edit dialog contains one `<form id="subscription-form">` outside `#form` (design D2); create and update return the region with the new values; invalid URL returns 422 with the editor and the field error; verify with `cargo test -p df-web project_subscriptions`
- [x] 2.2 Tests first: the row renders the delete confirmation state (`x-data="{ confirming: false }"`, text "Delete this subscription? Deliveries to it stop at once.", Cancel, Delete with `hx-delete`); the trash button and Cancel carry no `hx-delete`, so neither sends a DELETE; and the empty list renders "No subscriptions yet" with "New subscription" (design D4); implement; verify with `cargo test -p df-web project_subscriptions`
- [x] 2.3 Update `crates/df-e2e/tests/lifecycle_events.rs` to open the project dialog, choose the tab, create a subscription by pressing Enter in the URL field, cancel the dialog and check the subscription still exists; verify with `just e2e-local` (container-backed; state in the report if it could not run)

## 3. Tabs on phones

- [x] 3.1 Tests first: the tab list has the horizontal overflow classes below `md`, and `tabs.js` (read with `crate::assets::script_source`) scrolls the selected tab into view on init and change (design D5); implement; verify with `cargo test -p df-web` and a Playwright check against "Phone · edit project sheet, lifecycle events" (done by hand at 300 px, because the three tabs fit at 390 px until the "Context" tab exists; an automated 390 px check is TD-104)

## 4. Docs and design mapping

- [x] 4.1 Update `docs/content/integrations/lifecycle-events.md` (table row "Managed in", "Delivery status", the public key sentence and the how-to steps: Projects → edit → tab "Lifecycle events", subscriptions save on their own, delete asks first) and `docs/content/authentication/keycloak.md` (where admins find subscriptions); verify with `just docs-build`
- [x] 4.2 Add the new boards to "Mapping views to boards" in `docs/design-workflow.md` (route `/projects/<key>/edit`, tab Lifecycle events, data per board, viewport 1440 or 390); verify that every board name matches PenPot

## 5. Integration checks

- [x] 5.1 Compare the desktop boards and the phone board with the running UI (`just serve-local`, Playwright) per `docs/design-workflow.md`; fix defects or record deviations in its "Known deviations"
- [x] 5.2 `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `bazel --bazelrc=.bazelrc.agents test //...` (`just bazel-test`) pass; `openspec validate project-dialog-lifecycle-events --strict` passes
