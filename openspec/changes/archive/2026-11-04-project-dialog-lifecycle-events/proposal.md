# Proposal

## Why

Lifecycle event subscriptions live on a separate page that only a row action on the project list opens. Every other project setting is a tab of the project edit dialog, so the separate page is hard to find and has no design. The PenPot design now shows the subscriptions as a tab of that dialog.

## What Changes

- The project edit dialog gets the tab "Lifecycle events" after "General" and "Push checks", shown to users with the `admin` role only. A planned "Context" tab goes before it, so the tabs before it keep their positions for every role.
- The tab shows the public key panel (or the "feature off" note, and the development-build warning), the subscription list with last delivery and an "New subscription" action.
- Creating and editing a subscription happens in a form inside the tab, not in a second modal. Subscriptions save on their own; "Save project" does not apply to them.
- Deleting a subscription asks for confirmation in its row.
- **BREAKING** The page `/projects/{key}/subscriptions` and the "Lifecycle events" row action of the project list are removed.
- Below 768 px the dialog's tab row scrolls horizontally and keeps the open tab in view, because four tabs do not fit 390 px.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `event-subscriptions`: project subscriptions, their delivery status and the public key move from a separate settings page to the "Lifecycle events" tab of the project edit form; inline forms and delete confirmation. The capability comes from the unarchived change `lifecycle-events`, which must be archived first.
- `configuration-management`: the tab row of a configuration form scrolls horizontally below 768 px and keeps the open tab in view.

## Impact

- `crates/df-web`: `project_subscriptions.rs` (routes return tab fragments, no page), `config_pages.rs` (tab in the project form, row action removed), `assets/tabs.js` (scroll the open tab into view), tests.
- `crates/df-e2e/tests/lifecycle_events.rs`: opens the tab instead of the page.
- Docs: `docs/content/integrations/lifecycle-events.md`, `docs/content/authentication/keycloak.md`; `docs/design-workflow.md` board mapping.
- Design: PenPot "App Design", page "Config - Projects", boards "Projects · edit modal, lifecycle events" and its variants, "Phone · edit project sheet, lifecycle events".

Out of scope:
- The "Context" tab itself: designed, not built; it lands with `iter3-project-context` (iteration 3). That change places its tab before "Lifecycle events".
- Instance subscriptions in the UI: rejected (they stay configuration only, `event-subscriptions` spec).
