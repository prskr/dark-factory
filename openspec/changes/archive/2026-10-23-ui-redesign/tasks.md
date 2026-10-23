# Tasks

## 1. Titles in the domain and database

- [x] 1.1 Add title sanitizing and placeholder derivation to `df-core` (control characters and newlines to spaces, collapse, trim, 80 characters; first sentence with `…`) test-first; verify unit tests for the scenarios "Title with a line break" and "Placeholder at creation" and for empty input
- [x] 1.2 Add `TaskStatus::group()` (Active, Waiting, Done) to `df-core`; verify a unit test covering every status
- [x] 1.3 Add the migration for `tasks.title_source` (existing rows `user`) and `df_db::set_task_title` with the precedence in one conditional `UPDATE`; verify `df-db` tests for every source pair, including "Agent title after a rename", and regenerate `.sqlx/` if macros changed
- [x] 1.4 Make the title optional in `df_db::create_task` (placeholder from the description) and change the `task.assigned` prompt to ask for `set_task_title` when there is no user title; verify the scenarios "Task created" and "Prompt asks for a title"
- [x] 1.5 Add the rename operation (sanitize, source `user`, audit `task.rename`, `task_changed`); verify the scenario "Rename DF-15" in a `df-db` test

## 2. Factory tool set_task_title

- [x] 2.1 Add `set_task_title` to `contract/mcp/factory-tools.v1.json` and to the tools offered in `Assign`; verify the contract test that loads the schema
- [x] 2.2 Execute `set_task_title` in `df-agent-channel` (stored call, audit, `task_changed`, no-op result when a title is kept, error on empty title) test-first; verify the three scenarios of `specs/factory-tools`

## 3. Design tokens, assets and components

- [x] 3.1 Define the semantic tokens of design D1 for both themes in `styles/input.css` (`@theme` + `prefers-color-scheme`) and regenerate `assets/app.css`; verify the stylesheet test asserts both theme blocks and that a page has no hard-coded palette classes left in `layout.rs`
- [x] 3.2 Vendor Inter and JetBrains Mono (`woff2`, Latin subset, licence files) and the Lucide icon subset as `icon(Icon::…)`; verify an asset test that fonts are served with cache headers and that rendered pages reference no third-party host
- [x] 3.3 Replace `ui.rs` with the `components` module of design D2 (actions, badges, forms, overlay, lists, transcript, shell) test-first; verify an HTML test per component for accessible names, 44 px touch targets on phones and the status badge colors per status
- [x] 3.4 Write `docs/design-system.md` (principles, tokens with contrast ratios, type, spacing, breakpoints, components with signatures and do/don't, patterns, review checklist) and link it from `crates/df-web/README.md`; verify every component of 3.3 and every token of 3.1 is documented

## 4. App shell

- [x] 4.1 Rebuild `layout()` with the rail (desktop) and the bottom bar (phone) from the shell components; verify the scenarios "Rail on the task page", "Task list on a phone" and "Transcript on a phone" and the existing "Phone navigation" scenario of `configuration-management` in rendered-HTML tests
- [x] 4.2 Check theme and accessibility rules on the shell: no flash of the wrong theme (no theme script, CSS only), visible focus ring, accessible names of icon buttons; verify the scenarios "Icon-only button" and "Dark system setting" (CSS contains the dark block) in tests

## 5. Task workspace

- [x] 5.1 Render `/tasks` and `/tasks/{key}` from one workspace template with `data-view` and the breakpoint rules of design D4; verify HTML tests for both views and the scenario "Selecting another task on desktop" (the swap targets and `hx-push-url`)
- [x] 5.2 Task list with status chips and the project menu with search (filters in the URL, applied in `df-db` and in `/tasks/events`); verify the scenarios "Filter by project", "Project search" (menu markup and Alpine filter) and "Live update respects filters" in an SSE test
- [x] 5.3 Transcript column in the new style (messages, tool-call rows, pending messages, message input) using the transcript components; verify the existing transcript snapshot tests are updated deliberately and the HTML-escaping and streaming tests still pass
- [x] 5.4 Details column, slide-over and bottom sheet (status line, PR and issue, project, persona, task branch `factory/task/<KEY>`, runs with token usage); verify the scenarios "Rate-limited run", "PR opened" and "Details on a phone"
- [x] 5.5 Title display and inline rename in the transcript header (`POST /tasks/{key}/title`); verify the scenarios "Placeholder in the list" and "Rename" including the live list update

## 6. New task composer

- [x] 6.1 `GET /tasks/compose` and the composer dialog/sheet (project chip, description, options menu with linked issue chip, assignee, send); remove `/tasks/new`; verify the scenarios "Create DF-1", "Missing description", "Linked issue set" and "New task on a phone"
- [x] 6.2 Defaults and keyboard: current project filter or last used project and persona (`localStorage`), `Ctrl/Cmd+Enter` submits, Escape closes; verify the rendered defaults in a test

## 7. Configuration pages

- [x] 7.1 Restyle projects, personas and credentials (lists and forms) with the components; verify all existing `config_pages` tests pass unchanged and the scenario "Projects table on a phone"

## 8. Integration checks

- [x] 8.1 Verify `cargo clippy --all-targets -- -D warnings`, `cargo test -p df-web -p df-db -p df-core -p df-agent-channel`, the Bazel targets of these crates and `cargo llvm-cov -p df-web` ≥ 70 % line coverage (Bazel: the container-backed `df_web_test` and `df_db_test` cases need the socket; all five targets passed on 2026-10-23 with `--test_env=DOCKER_HOST=unix:///run/user/501/podman/podman.sock --sandbox_writable_path=/run/user/501/podman`)
- [ ] 8.2 Check desktop (1440 and 1100 px) and phone (390 px) in a real browser in light and dark mode against the mockup: navigation, task switching, filters, composer, rename, details sheet, reconnect after sleep; record the result in the PR description
