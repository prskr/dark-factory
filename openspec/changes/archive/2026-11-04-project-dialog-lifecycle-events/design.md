# Design

## Context

- `project_subscriptions.rs` serves a full page `/projects/{key}/subscriptions` (admin only, `require_admin` route layer) with a table and modal forms (`new_form`, `edit_form`, `save`, `remove`). The project list links to it with a row action (`settings_link`).
- The project edit form is `tabbed_form` in `config_pages.rs`: one `<form id="form">` around all tab panels, swapped as a whole (`hx-swap="outerHTML"`) on a validation error. The Alpine component `tabs` (`assets/tabs.js`) switches panels. Below 768 px the tab list is a row without overflow handling.
- Design: PenPot "Config - Projects", boards "Projects · edit modal, lifecycle events" (list, new subscription, empty, feature off, invalid URL, unrestricted URLs, delete subscription) and "Phone · edit project sheet, lifecycle events".

## Goals / Non-Goals

**Goals:**
- Subscriptions managed in the project dialog without a nested modal and without coupling them to "Save project".
- Keep the existing subscription endpoints, validation and admin guard.

**Non-Goals:**
- Editing subscriptions together with the project in one submit: rejected (repeated nested fields in the project form, partial failure handling, for no user gain).
- A "Context" tab: `iter3-project-context` (iteration 3).

## Decisions

### D1 The tab loads its content on first open

The tab button is rendered for admins only. Its panel holds a placeholder with `hx-get="/projects/{key}/subscriptions"` triggered once when the panel is shown first. The handler checks `require_admin` first (403 for anyone else, with or without `HX-Request`), then returns the tab fragment (key panel, header, list or empty panel) for htmx requests and redirects plain requests to `/projects`.

- Why: `project_edit` and the re-render after a validation error (`project_update`) stay free of subscription queries; a project re-render does not refetch subscriptions; the admin guard stays on one router.
- Alternative: render the subscriptions with the dialog. Rejected: every project form render (also for board members' validation errors) would need the subscription list and the signing state.

### D2 The subscription form is not nested in the project form

The tab sits inside `<form id="form">`, and HTML forbids nested forms. The inline editor's inputs and buttons carry `form="subscription-form"`, which associates them with one separate, empty `<form id="subscription-form">` rendered after the project form in the dialog (admins only). The editor's submit posts that form to the create or update endpoint (`hx-post` on the submit button) and swaps the subscriptions region.

- Effect: Enter in the URL field submits the subscription, never the project; "Save project" does not send `url` or `events`.
- Alternative: no form element, `hx-include` of the editor fields. Rejected: the inputs would belong to the project form, so Enter would submit the project and "Save project" would carry the fields.

### D3 Responses swap the subscriptions region

`new` and `edit` return the whole region (`#lifecycle-events`) with at most one editor open: the new editor above the list, or the edit editor in place of its row. Opening one closes the other and discards its unsaved values. A successful create, update or delete returns the whole region (`#lifecycle-events`), which also refreshes the count. A validation error returns the editor with status 422 and the values kept, as the other forms do.

### D4 Delete confirmation is client state of the row

The row holds `x-data="{ confirming: false }"` (plain data, allowed inline by `crates/df-web/AGENTS.md`). The trash button sets it; the confirm state shows the text and "Cancel" / "Delete"; only "Delete" sends `hx-delete`. No server round trip for the confirmation.

### D5 Tab row on phones

The tab list gets `overflow-x-auto` below `md` and `shrink-0` tabs. `tabs.js` calls `scrollIntoView({ block: "nearest", inline: "nearest" })` on the selected tab on init and on every change. Applies to every `tabbed_form`.

## Risks / Trade-offs

- [An admin cancels the dialog and expects a new subscription to be undone] → The tab's intro says that subscriptions save on their own (spec "Subscriptions save on their own").
- [htmx attribute resolution for `form=` on the submit button differs from plain HTML] → Covered by a browser test (e2e) that submits with Enter and with the button.
- [The e2e test and docs link to the old page] → Updated in the same change.

## Migration Plan

No data change. Old bookmarks of `/projects/{key}/subscriptions` redirect to `/projects`. Rollback: revert the change.
