# Tasks

Write each test first and watch it fail (TDD). Then write the code. Run tests with `cargo nextest run -p df-web` (skill `rust`) and check TypeScript with `npx --yes -p typescript@<version> tsc -p crates/df-web` (version as in `lefthook.yml`; "`tsc`" below). Tests that need PostgreSQL run only with a container socket; otherwise CI runs them. Name the ones you could not run. E2e tests follow skill `e2e`; CI runs them if no socket exists.

## 1. `htmx` module: `Region`, `partial`, `is_htmx`

- [x] 1.1 Write unit tests in a new `crates/df-web/src/htmx.rs`: the IDs in `Region::ALL` are unique and valid HTML IDs (not empty, no whitespace, no `#`), and `partial("#x", "innerHTML", html!{…})` renders the same markup as today's `<hx-partial hx-target=… hx-swap=…>`. Then add the `regions!` macro (enum, `ALL`, `const fn id`, `const fn selector`), `partial(target, swap, inner: impl Render)` and `is_htmx` (moved from `auth/mod.rs`, callers updated). Verify that the tests pass.
- [x] 1.2 Remove the `partial` copies in `sse.rs`, `requests.rs` and `egress/log/viewer.rs`, and `partial_str` (callers pass `PreEscaped` and call `.into_string()`). Verify that the existing `sse`, `requests` and `egress` tests pass unchanged.
- [x] 1.3 Make each element ID a `Region` when a selector names it in a different file from its `id=`, or when `sse.rs` names it (design, Decision 4). Use the enum for both `id=` and the selector. This includes `egress/project_tab.rs`'s `REGION` and `SEND_SHORTCUT` (becomes `send_shortcut()`). Leave IDs that only one file uses literal. Verify: for each `"#…"` literal that remains in non-test code, `grep -rlF '<id>' crates/df-web/src --include=*.rs` lists only the file that sets it (or it is dynamic or a colour). The full `df-web` suite passes.
- [x] 1.4 Add to `crates/df-web/AGENTS.md`: shared element IDs come from `htmx::Region`, `<hx-partial>` goes through `htmx::partial`, and page-local IDs stay literal. Verify by reading the section against Decision 4.

## 2. Toast region, frame and answer

- [x] 2.1 Write unit tests for `components::toast`, `components::error_toast` and `components::toast_answer`: text, tone icon, and a Dismiss button with the accessible name "Dismiss". The answer is one `<hx-partial hx-target="#toasts" hx-swap="innerHTML">` with `HX-Reswap: none` and the given status. Then implement them (frame from `egress/log/toast.rs::created`, no live role on the toast). Verify that the tests pass.
- [x] 2.2 Write a layout test: every page has exactly one `Region::Toasts` element with `role="status"` and `aria-live="polite"`, the `Region::NetworkErrorToast` template, and `data-toasts`/`data-network-error` on `<body>`. Then render them in `layout()` (fixed corner, above the phone bottom bar). Verify that the test passes. With a container socket (`just serve-local`), also check in the browser: the bottom bar does not cover a toast at 390 px width (the offset is unverified), and a screen reader announces an error toast once (nested live regions, review F14). Otherwise state that these checks are open. Status: the 390 px placement is covered by an e2e test; the screen-reader check cannot be automated and is open and manual.
- [x] 2.3 Update the tests in `egress/log/page.rs`, `egress/tests/allow.rs` and `egress/tests/deny_log.rs` to expect the shared region and the partial answer (no `HX-Retarget`, no `#allow-toast`). The deny error has a Dismiss button. Then move the traffic page over: `into_toast` is replaced by `toast_answer`, `deny.rs::toast_error` by `error_toast("Rule not created", …)`, the deny button loses `hx-target="#allow-toast"` and gets `hx-swap="none"`, and `#allow-toast` is removed from `egress/log/page.rs`. Verify that the tests pass and that the e2e test `crates/df-e2e/tests/egress_rules.rs` still passes.

## 3. Error answers for htmx requests

- [x] 3.1 Write the failing tests in `htmx.rs` with a small router behind the middleware. With `HX-Request: true`, `not_found` and `internal_error` answer with:
  - the same status, `HX-Reswap: none`, `HX-Push-Url: false` and `Content-Type: text/html; charset=utf-8`;
  - a body that is one `<hx-partial hx-target="#toasts" hx-swap="innerHTML">` with "404 Not found" (or "500 Error") and the message, and no `<html`;
  - a `Set-Cookie` header that the inner handler set, which is still there.

  Without the header, and with `HX-Request: true` plus `HX-History-Restore-Request: true`, they answer with the full page. `responses::invalid` (422) and a 409 answer pass through unchanged. An SSE response with `HX-Request` keeps its `text/event-stream` content type and body.
- [x] 3.2 Add the `ErrorAnswer` extension to `error_page` and implement `htmx::error_answers`. It keeps the inner response's parts and headers and replaces only the body. Layer it as the outermost layer of the `ui` router in `lib.rs`. Verify that the tests of 3.1 pass.
- [x] 3.3 Write router-level tests:
  - `DELETE /tasks/DF-404/messages/abc` (htmx, board member) answers 404 with the toast;
  - `GET /tasks/DF-404` without `HX-Request` answers with the error page;
  - a subscription route as a non-admin with `HX-Request` answers 403 with the toast;
  - the "make global" post of the project egress tab with an invalid port and `HX-Request` answers 400 with the toast.

  Then convert `egress/project_tab.rs`'s empty 400 to `error_page`. Verify that the tests pass (PostgreSQL-backed ones with a socket, else CI).
- [x] 3.4 Document the rule. In the module docs of `responses.rs` and in `crates/df-web/AGENTS.md`: error answers go through `auth::error_page` (the toast for htmx requests depends on it), and toasts go through `components::toast_answer`. Verify with `cargo doc -p df-web --no-deps` (no warnings).
- [x] 3.5 Add a short section "When something fails" to `docs/content/using/tasks.md`, and a pointer to it in `docs/content/using/chats.md`: a failed action shows a message in the corner, the page stays as it was, and a reload shows the full error page. Verify with `just docs-build`.

## 4. Network errors

- [x] 4.1 Write a test with `script_source("htmxErrors.ts")`: it listens to `htmx:error`, checks `ctx` and `!ctx.response` (`response` must appear in the check), reads `data-toasts` and `data-network-error`, and contains no `#` ID literal. Then add `scripts/htmxErrors.ts` and extend `scripts/types/*.d.ts` if needed. Add the script to `COMPONENT_SCRIPTS`. Name it as a plain (non-Alpine) script in `crates/df-web/AGENTS.md`, next to `sse-cleanup.ts`. Verify that the test and `tsc` pass.
- [x] 4.2 Write an e2e test in `crates/df-e2e/tests/ui.rs`: with the task page open, abort the message post through Playwright `route.abort`, send a message, and check that the network toast appears and the input keeps its text. Verify that it passes.
- [x] 4.3 Add one sentence on the network toast to the "When something fails" section of 3.5. Verify with `just docs-build`.

## 5. Inline script code to TypeScript

- [x] 5.1 Write tests: the composer page has `x-data="composer({`, no `x-init` and no `localStorage`, and `script_source("composer.ts")` has `Alpine.data('composer'`, `localStorage` inside `try` and the `data-explicit` check. Then add `scripts/composer.ts` and render `x-data="composer(…)"` from JSON in `task_pages/view.rs`, without the `x-init` and `x-on:submit` IIFEs. Verify that the new tests, the composer tests in `task_pages/tests/create.rs` and `tsc` pass.
- [x] 5.2 Write tests: `transcript_scroll` renders `x-data="transcriptScroll"` and no `x-init`, and `script_source("transcriptScroll.ts")` creates `MutationObserver` and `ResizeObserver` and disconnects them in `destroy`. Then add `scripts/transcriptScroll.ts` and change `task_pages.rs::transcript_scroll`. Verify that the tests, `tsc` and the e2e tests in `crates/df-e2e/tests/transcript.rs` pass (the transcript keeps following the latest item).
- [x] 5.3 Write tests: the task row has `x-on:click="select($el)"` and no `querySelectorAll`, `#task-rows` and `#chat-rows` carry `data-rows`, and both `workspace.ts` and `chatLayout.ts` define `select(row` with `closest('[data-rows]')`. Then implement it. Verify that the tests, `tsc` and the e2e test `opening_tasks_from_the_list_keeps_the_console_clean_and_the_details_tab` pass.
- [x] 5.4 Update the layout tests in `layout.rs` (the `x-on:popstate.window` assertions) to expect `x-data="pageView"` and `x-on:popstate.window="syncView()"` on workspace pages only. Then add `scripts/pageView.ts`. Verify that the tests and `tsc` pass.
- [x] 5.5 Add the new scripts to `COMPONENT_SCRIPTS`. Verify that no script code is left in Rust strings: `grep -rnE 'localStorage|Observer\(|\.test\(|\}\)\(\)|querySelectorAll' crates/df-web/src --include=*.rs` finds hits only in `#[cfg(test)]` code and in comments.

## 6. Accessibility after swaps

- [x] 6.1 Write a test on `render_transcript`: `#transcript` has `role="log"` and `aria-live="polite"`. Then add the attributes in `transcript/mod.rs`. Verify that the test passes.
- [x] 6.2 Write tests for `task_pages::view::head_region` and `chat_pages::view::head_region`: the head has `tabindex="-1"` and `aria-labelledby="detail-title"` and a `focus-visible` ring class, and its first child is `h2 id="detail-title" class="sr-only"` with key and title. Then implement it. Verify that the tests pass.
- [x] 6.3 Write tests: task row links have `data-focus-after-swap="#task-head"`, chat row links have `"#chat-head"`, and `script_source("focusAfterSwap.ts")` checks `status < 400`. Then add `scripts/focusAfterSwap.ts` and the attribute, add the script to `COMPONENT_SCRIPTS`, and name it as a plain script in `crates/df-web/AGENTS.md`. Verify that the tests and `tsc` pass.
- [x] 6.4 (Note: the failed-selection e2e test uses a real 404 from the server, which goes through the same `error_answers` path as a 500; the unit tests in `htmx/tests.rs` cover the 500.) Extend the e2e test `opening_tasks_from_the_list_keeps_the_console_clean_and_the_details_tab`:
  - after a keyboard activation of a row, the focused element is the head of that task, and it has a visible focus outline (computed style);
  - after `page.route` fulfils the next row's GET with a 500 `error_page` answer, the URL is unchanged, the toast is shown and focus stays on the row.

  Verify that it passes.

## 7. Final checks

- [x] 7.1 Run `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `tsc` and `openspec validate web-htmx-robustness --strict`. All pass.
