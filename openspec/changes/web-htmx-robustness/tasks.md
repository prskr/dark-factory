# Tasks

Write each test first (TDD). Run tests with `cargo nextest run -p df-web` (skill `rust`). Tests that need PostgreSQL run only with a container socket; otherwise CI runs them. Name the ones you could not run.

## 1. `htmx` module: `Region`, `partial`, `is_htmx`

- [ ] 1.1 Create `crates/df-web/src/htmx.rs` with `Region` (`id()`, `selector()`, `ALL`), `partial(target, swap, inner: impl Render)` and `is_htmx` (moved from `auth/mod.rs`, callers updated). Add unit tests: unique IDs, `selector() == "#" + id()`, and `partial` output equal to today's `<hx-partial hx-target=… hx-swap=…>` markup. Verify that the tests pass.
- [ ] 1.2 Remove the `partial` copies in `sse.rs`, `requests.rs` and `egress/log/viewer.rs` and `partial_str` (callers pass `PreEscaped`). Verify that `cargo nextest run -p df-web sse requests egress` passes unchanged.
- [ ] 1.3 Replace every fixed region ID that a selector in Rust code names (`hx-target`, `hx-select`, `hx-select-oob`, `hx-include`, `<hx-partial>`, `HX-Retarget`, the `sse.rs` region list) with `Region` for both `id=` and the selector. This includes `egress/project_tab.rs`'s `REGION`. Verify with `grep -rnE 'hx-(target|select|select-oob|include)="#|"#[a-z][a-z-]+"' crates/df-web/src --include=*.rs`: the only hits left are tests, dynamic per-item IDs, colours and IDs that nothing selects. Then verify that the full `df-web` suite passes.

## 2. Toast region and error toast markup

- [ ] 2.1 Add `toast(kind, body)` and `error_toast(status, title, message)` to `components` (frame from `egress/log/toast.rs::created`, Dismiss button; the error toast has `role="alert"` and an alert icon). Add a unit test for the role, the text and the Dismiss button's accessible name.
- [ ] 2.2 Render the toast region (`Region::Toasts`, `role="status"`, `aria-live="polite"`, fixed corner above the phone bottom bar) and `template id=(Region::NetworkErrorToast.id())` in `layout()`. Fill `data-toasts`/`data-network-error` on `<body>`. Add a layout test that every page has one region and the template. In the browser (`just serve-local` when a socket exists), check that the bottom bar does not cover the toast at 390 px width (the offset is unverified).
- [ ] 2.3 Move the traffic page to the shared region: `into_toast` targets `Region::Toasts`, and `#allow-toast` is removed from `egress/log/page.rs`. Update the tests in `egress/log/page.rs`, `egress/tests/allow.rs` and `egress/tests/deny_log.rs` (no e2e test names `#allow-toast`). Verify that the tests pass.

## 3. Error answers for htmx requests

- [ ] 3.1 Write the failing tests in `htmx.rs` with a small router behind the middleware: `not_found` and `internal_error` with `HX-Request: true` answer with the same status, `HX-Reswap: none`, `HX-Push-Url: false` and a body that is one `<hx-partial hx-target="#toasts" hx-swap="innerHTML">` with title and message and no `<html`. Without the header, and with `HX-History-Restore-Request: true`, they answer with the full page. `responses::invalid` (422) and a 409 answer pass through unchanged. An SSE response with `HX-Request` keeps its `text/event-stream` content type and body.
- [ ] 3.2 Add the `ErrorAnswer` extension to `error_page` and implement `htmx::error_answers`. Layer it as the outermost layer of the `ui` router in `lib.rs`. Make 3.1 pass.
- [ ] 3.3 Add router-level tests: `DELETE /tasks/DF-404/messages/abc` (htmx, board member) answers 404 with the toast. `GET /tasks/DF-404` without `HX-Request` answers with the error page. A subscription route as a non-admin with `HX-Request` answers 403 with the toast. Verify that the tests pass (PostgreSQL-backed tests: with a socket, else CI).
- [ ] 3.4 State in the module docs of `responses.rs` that error answers go through `auth::error_page` (the toast for htmx requests depends on it).

## 4. Network errors

- [ ] 4.1 Add `scripts/htmxErrors.ts`: on `htmx:error` with `detail.ctx`, clone the template named by `data-network-error` into the region named by `data-toasts`. Extend `scripts/types/*.d.ts` if the types need it, and add the script to `COMPONENT_SCRIPTS`. Add a test with `script_source` that checks the event name, the `ctx` check and that no ID literal is in the file. Verify with `npx --yes -p typescript@<version> tsc -p crates/df-web`.
- [ ] 4.2 Add an e2e test in `crates/df-e2e/tests/ui.rs` (skill `e2e`): with the task page open, make the browser abort the message post (Playwright route abort), send a message, and check that the network toast appears and the input keeps its text. CI runs it if no socket exists.

## 5. Inline script code to TypeScript

- [ ] 5.1 Add `scripts/composer.ts` (`composer({ issue, branch })`, `init()` preselects from `localStorage['df.composer']` unless `data-explicit` is set or the value is not offered, `remember()` on submit, both in `try/catch`). Render `x-data="composer(…)"` from JSON in `task_pages/view.rs` and remove the `x-init` and `x-on:submit` IIFEs. Add tests: the page has `x-data="composer({` and no `localStorage`, and the script source has `Alpine.data('composer'`. Verify with the composer tests in `task_pages/tests/create.rs` and with `tsc`.
- [ ] 5.2 Add `select(row)` to `workspace.ts`, and switch `chatLayout.select` to `row.closest('ul')`. The task row uses `x-on:click="select($el)"`. Add a test that the row has no `querySelectorAll` and that both script sources define `select(row`. Verify with `tsc` and the e2e test `opening_tasks_from_the_list_keeps_the_console_clean_and_the_details_tab`.
- [ ] 5.3 Add `scripts/pageView.ts` with `syncView()`. The body uses `x-data="pageView"` and `x-on:popstate.window="syncView()"` on workspace pages only. Update the layout tests at `layout.rs` (`x-on:popstate.window` assertions). Verify that the tests pass and `tsc` passes.
- [ ] 5.4 Check that no `x-init`, `x-on:*` or `x-data` attribute left in `crates/df-web/src` holds an IIFE, a `localStorage` access, a regex or a DOM query: `grep -rnE '\(\(\) =>|localStorage|querySelector|\.test\(' crates/df-web/src --include=*.rs` finds only test code.

## 6. Accessibility after swaps

- [ ] 6.1 Add `role="log"` and `aria-live="polite"` to `#transcript` in `transcript/mod.rs`. Add a test on `render_transcript`.
- [ ] 6.2 Add the sr-only `h2 id=(Region::DetailHeading.id()) tabindex="-1"` with key and title to `task_pages::view::head_region` and `chat_pages::view::head_region`. Add tests for both heads.
- [ ] 6.3 Add `scripts/focusAfterSwap.ts` (on `htmx:after:swap`: source element with `data-focus-after-swap` and `ctx.response.status < 400` → focus the selector) and add it to `COMPONENT_SCRIPTS`. Give task and chat row links `data-focus-after-swap=(Region::DetailHeading.selector())`. Add tests: the rows have the attribute, and the script source checks the status. Verify with `tsc`.
- [ ] 6.4 Extend the e2e test `opening_tasks_from_the_list_keeps_the_console_clean_and_the_details_tab`: after a row click, the focused element is the detail heading of the clicked task.

## 7. Docs and checks

- [ ] 7.1 Add a short section to `docs/content/using/tasks.md` (and a pointer in `docs/content/using/chats.md`): a failed action shows a message in the corner, and the page stays as it was. Verify with `just docs-build`.
- [ ] 7.2 Update `crates/df-web/AGENTS.md`: region IDs come from `htmx::Region`, `<hx-partial>` goes through `htmx::partial`, and error answers go through `auth::error_page`. Name the plain (non-Alpine) scripts next to `sse-cleanup.ts`.
- [ ] 7.3 Run `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, the `tsc` checks and `openspec validate web-htmx-robustness --strict`. All pass.
