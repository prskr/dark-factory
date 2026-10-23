# Tasks

## 1. Queued messages in the database

- [ ] 1.1 Add the migration for `task_messages.forwarded_at` (nullable, set to `now()` for rows undelivered at migration time, design Migration Plan) and a `df-db` claim function that marks held-back undelivered messages of a task as forwarded in queue order and returns them; verify a `df-db` test that a second claim returns nothing, that delivered messages are never claimed and that a message undelivered before the migration is not editable, and regenerate `.sqlx/`
- [ ] 1.2 Add `edit_queued_message` and `remove_queued_message` (design D2: one conditional write each, human authors only, audit `message.edit` / `message.remove`, `task_message_changed` notification, removal returns a `queued` task with no undelivered message left to `waiting` in the same transaction) test-first; verify `df-db` tests for the `task-management` scenarios "Edit before forwarding", "Edit after forwarding", "Remove before forwarding", "Assignment prompt", "Empty edit", "Last message of a waiting task removed" and "Other messages still queued"

## 2. Forwarding between turns

- [ ] 2.1 Track `busy` per session from persisted `TurnStarted`/`TurnEnded` of its run and derive it at session start (design D1); verify `df-agent-channel` tests for a fresh run, an open turn at reconnect and a run that ends during a turn
- [ ] 2.2 Change `send_new_prompts` to re-send forwarded undelivered prompts and to claim and send held-back prompts only when not busy, and call it after a batch with `TurnEnded`; verify tests for the scenarios "Message during a turn", "Message on an idle run" and "Reconnect after forwarding", plus the existing session tests
- [ ] 2.3 Document the hold-back in the agent-channel docs (when a queued message reaches the runtime); verify the doc names `forwarded_at` and the turn events

## 3. Shell, logo and navigation

- [ ] 3.1 Add the factory mark as `assets/logo.svg` and a logo icon component; link it as SVG favicon in `layout` and show it at the top of the rail linking to `/tasks`; verify the `web-ui-shell` scenarios "Favicon" and "Logo in the rail" in rendered-HTML tests and that the asset is served with cache headers
- [ ] 3.2 Rebuild the rail as in the mockup: icon-only links with the mockup's icons, section names as `aria-label` and `title`, the active bar at the edge, no visible labels; verify the scenarios "Rail on the task page" and "Section names as tooltips"
- [ ] 3.3 Change the phone bottom bar to Tasks / create / Projects with the create action per page (New project on Projects, New task elsewhere) and remove Log out from it; verify the scenarios "Task list on a phone", "Projects on a phone", "Transcript on a phone" and the `configuration-management` scenario "Phone navigation"
- [ ] 3.4 Add the phone profile menu (avatar with the session user's initials top right on the task list and Projects, menu with name, role and log out); verify the scenario "Log out on a phone" at HTML level (menu holds the `hx-post="/logout"` action, avatar has an accessible name) and the 44 px targets
- [ ] 3.5 Document logo, rail tooltips, bottom bar create action and profile menu in `docs/design-system.md`; verify the docs test that lists components passes

## 4. Task list and workspace

- [ ] 4.1 Move the project menu into the "Tasks" title row from 1024 px, drop it below 1024 px and show an active project filter as a removable chip on phones; verify the `task-ui` scenarios "Filter by project", "Project search", "Live update respects filters" and "Project filter on a phone"
- [ ] 4.2 Add the collapsible details column (design D9: hide button in the details header, 4 rem strip with show button, status dot and PR/issue links with tooltips, state in `localStorage` with `try/catch`); verify an HTML test for both controls and their accessible names, and note the reload check for task 7.2
- [ ] 4.3 Add the empty states of the task list ("No active tasks", "Show all tasks") and of the workspace without a selected task ("No task selected", New task); verify the scenarios "No active tasks" and "Workspace without selection"
- [ ] 4.4 Add explicit Save and Cancel to the inline rename in the task header; verify the scenarios "Rename" and "Cancel rename"
- [ ] 4.5 Restyle the new-task composer as in the mockup (no solid frame, chips above a rounded card, round icon submit with accessible name "Create task", options and assignee inside the card); verify the existing composer scenarios still pass and an HTML test for the submit button's accessible name
- [ ] 4.6 Document empty state, collapsed details strip and composer changes in `docs/design-system.md`; verify the docs test passes

## 5. Conversation transcript and queued stack

- [ ] 5.1 Render transcript items as a conversation (design D4: `data-author`, bubbles with the new tokens in both themes, meta under the bubble, avatars hidden by CSS for all but the last item of a group, borderless one-line tool rows, inline code chips, run dividers) test-first; verify the `task-ui` scenarios "Agent writes twice" and "Second run" in HTML tests, update the `insta` snapshots, and check the new token pairs for 4.5:1 contrast in `docs/design-system.md`
- [ ] 5.2 Move the pending messages into a queued stack above the message input with queue time, edit and remove for held-back messages and none for forwarded ones; verify the scenarios "Message while the agent works", "Follow-up message" and an SSE test for "Message picked up"
- [ ] 5.3 Add `PATCH` and `DELETE /tasks/{key}/messages/{id}` (409 with "The agent has already received this message." when forwarded, 422 on empty text) and re-render the stack on `task_message_changed` over the task's SSE stream; verify handler tests for both outcomes and the scenario "Remove"
- [ ] 5.4 Add editing in the message input (Alpine state per design D3: load text, "Editing a queued message" bar with Cancel, Save instead of Send, highlighted stack item, draft restored on Cancel); verify the scenarios "Edit and save" and "Forwarded while editing" in HTML/handler tests
- [ ] 5.5 Document bubble, run divider, queued stack and editing bar in `docs/design-system.md`; verify the docs test passes

## 6. Configuration pages

- [ ] 6.0 Add the URL-only status filters `queued` and `running` to the task list and `/tasks/events` (design D6); verify the `task-ui` scenario "Narrower filter from a link" including the live update
- [ ] 6.1 Add `df-db::project_status_counts` and render the project table with forge, key, name, repository, branch and the Queued / Running / Waiting / Done counts as links (design D6), muted zeros, and a compact list with the counts below 1024 px; verify the scenarios "Counts of a project", "Count as link" and "Running count as link" and that the page has no horizontal scroll markup at 390 px ("Projects table on a phone")
- [ ] 6.2 Pass the configured forge integration from `dark-factory` into `WebState` and add `ForgeKind` with icons (Forgejo, GitHub, generic Git, generic issue; licence note for the Forgejo mark); verify the scenarios "Project served by the forgejo integration", "Project without forge credential" and "No forge integration"
- [ ] 6.3 Add the forge feedback fragment for the project form (design D5, `keyup changed delay:300ms`, icon inside the Repository URL and Issue tracker URL fields, `aria-live` hint); verify the scenarios "Typing a repository URL" and "Not an HTTPS repository" in handler tests
- [ ] 6.4 Render personas as cards (initial, name, harness · model or "default", three-line prompt excerpt, credential, edit button); verify the scenario "Persona card"
- [ ] 6.5 Turn the project, persona and credential forms into modal fragments (design D7: one `<dialog>`, fragment routes, 422 inside the modal, success re-renders the list and closes the modal, non-htmx GET redirects to the list, full-height sheet below 1024 px) and remove the full-page form routes; verify the scenarios "Validation error in the modal" and "Saved project", the existing create/edit/audit scenarios of `configuration-management`, and that the removed routes no longer render pages
- [ ] 6.6 Add the empty states of the project, persona and credential lists (explanation and create action, project list links to credential references); verify the scenario "No projects yet" and one test per list
- [ ] 6.7 Document modal/sheet forms, project table with counts, forge icon and persona card in `docs/design-system.md`; verify the docs test passes

## 7. Integration checks

- [ ] 7.1 Run fmt, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --workspace` (container tests with a socket, otherwise list the skipped ones), `cargo-machete` and the Bazel `df_web_test` target; verify all pass and `.sqlx/` is current
- [ ] 7.2 Check the UI in a real browser at 1440, 1100 and 390 px, light and dark (carried over from `ui-redesign` 8.2): live updates, task selection, phone Back, details sheet and collapse with reload, queued edit/remove during a turn, configuration modals, phone navigation and profile menu; record the result in the change

## Workflow follow-up

- Archive the change after the review rounds of `/implement-change` are done.
