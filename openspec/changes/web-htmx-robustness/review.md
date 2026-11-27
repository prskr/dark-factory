# Review: web-htmx-robustness

**Verdict:** FAIL. Revise the artifacts before implementation. The approach holds. One task cannot pass its own check (F1), and five more medium findings need artifact edits.

## Success condition tested

The change is ready to implement when:

- its claims about `crates/df-web/src` and `htmx.org@4.0.2` are correct;
- it follows `crates/df-web/AGENTS.md`, `CODING_STANDARDS.md` and `openspec/config.yaml`;
- the specs are testable behaviour (each requirement under 500 characters, WHEN/THEN in every scenario);
- the tasks are TDD-ordered, each has a verification step, and no test or doc work sits in a final group;
- an implementer can follow the tasks without running into a contradiction.

## Steps run

- `openspec validate web-htmx-robustness --strict`: `Change 'web-htmx-robustness' is valid`.
- Requirement lengths (awk over both spec files): 179, 304, 254, 332, 197, 255 and 202 characters, all under 500. I read every scenario: each has a WHEN and a THEN.
- `curl https://unpkg.com/htmx.org@4.0.2/dist/htmx.js` and `…/dist/ext/hx-sse.js` into `/tmp/htmx402`. I read `#createRequestContext`, `#issueRequest` (lines 560–630), `swap` (1267–1327), `#processMainSwap` (1329–1358), `#processOOB` (1111–1131), `#processPartials` (1153–1183), `#restoreHistory` and `#resolveHistoryAction` (1643–1700), `#handleStatusCodes` (2272), and the SSE `htmx_before_response` and reconnect loop.
- Grep checks over `crates/df-web/src` (all `--include=*.rs`, without test files unless stated):
  - the call sites of `error_page|not_found|internal_error`: 158;
  - the regex of task 5.4;
  - every `"#…"` literal and every `hx-target`/`hx-select`/`hx-include`;
  - every `StatusCode::` error answer that bypasses `error_page`;
  - `#allow-toast` in `crates/df-e2e`: no hit.
- I read the router (`lib.rs` 139–172), `csrf_guard`, `require_board_member`, `login_redirect`, `error_page` (`auth/mod.rs`), `responses.rs`, `egress/log/{toast,deny,page}.rs`, the task and chat row markup and heads, `chatLayout.ts`, `layout.rs` (`COMPONENT_SCRIPTS`, popstate) and the composer form (`task_pages/view.rs:1039–1042`).

## Verified as correct

- Every htmx fact in design "Context":
  - `noSwap: [204, 304]` (l. 203);
  - `HX-Retarget`/`HX-Reswap`/`HX-Reselect` are applied before the swap (l. 610–612);
  - partials are processed whatever the main swap style is, and a body with only partials skips the main swap (l. 1272–1278, 1335–1340);
  - the title is not changed when `ctx.swap` is `none` (l. 1322);
  - `HX-Push-Url: false` overrides `hx-push-url` (l. 1672–1683), and `HX-Request` is set on history restores;
  - `htmx:error` with `{ctx, error}`, except for `AbortError` unless it is a timeout (l. 622), and with `{error}` only at l. 381 and 1733;
  - `htmx:after:swap` fires on the source element, also for a `none` swap;
  - SSE drops fire `htmx:sse:error`, and the first SSE request goes through the normal pipeline (`htmx_before_response` takes over only for `text/event-stream`).
- **Decision 1 (partial + `HX-Reswap: none`, not `HX-Retarget`) is the right choice.** With `HX-Retarget` alone, the row links' `hx-select="#ws-detail"` (`task_pages/view.rs:196`, `chat_pages/view.rs:62`) filters the toast out of the fragment (l. 1341–1344). The partial needs no `HX-Reselect` and does not depend on the source element's attributes. One argument in the design is wrong (F10), but the decision stands.
- The paths in Impact exist: `auth::error_page` (`auth/mod.rs:179`), `auth::is_htmx` (`auth/mod.rs:401`), and the three `partial` copies plus `partial_str` (`sse.rs:569`, `requests.rs:598`, `egress/log/viewer.rs:342/346`). So do `egress/project_tab.rs`'s `REGION`, `transcript::render_transcript` (`transcript/mod.rs:713`), both `head_region`s, `assets::script_source`, `scripts/types/*.d.ts` and `sse-cleanup.ts`. The claim "about 157 call sites" is right (158). The 422 (`responses::invalid`) and the 409 answers do not use `error_page`. No e2e test names `#allow-toast`. Playwright `route.abort` is already used (`df-e2e/tests/markdown_editor.rs:163`).
- The task head has no heading and its rename button's `aria-label` hides the title. The chat head shows the title only below 768 px (`md:hidden`). The list columns own the `h1`.
- Scope is right: there are no JS-free forms, no CSRF changes and no component or token work. Item 5 (`#ws-detail` only) is rejected with a reason.

## Findings (most severe first)

### F1 (High): task 5.4 cannot pass, and the Goal "no script code in Rust strings" is not met

**Files:** `tasks.md` 5.4, `design.md` Goals and Decision 5.

**Problem:** I ran the task 5.4 regex on the current tree. These hits are not test code, and the change leaves them in place:

- `\(\(\) =>` matches every `$nextTick(() => …)`. Those are short inline expressions that AGENTS.md allows: `project_subscriptions.rs:373,385`, `components/forms.rs:395`, `components/transcript.rs:201`, `chat_pages/view.rs:263`, `task_pages/view.rs:349`.
- `task_pages.rs:234`: the transcript scroller's `x-init`. It defines a function and sets up a `MutationObserver` and two `ResizeObserver`s with `$el.querySelector('#transcript')`. This is script code under AGENTS.md, and the design does not mention it.
- `task_pages/view.rs:35`: `SEND_SHORTCUT`, which contains `document.querySelector('#composer-action [aria-label=Send]')`. The design says the shortcut "stays inline (short expressions)", but 5.4 forbids it.

**Fix:**

- Add the scroller to Decision 5, for example a `followLatest` component or a method on `workspace`, with its own task. Either move `SEND_SHORTCUT` to a method as well, or state that it stays.
- Change the 5.4 check so that it can pass. For example, grep only `x-init|x-on:submit|x-data` values for `localStorage|MutationObserver|=> \{|\.test\(`, or list the allowed hits.
- Or narrow the Goal to the sites this change moves.

### F2 (Medium): the middleware must keep the inner response's headers

**Files:** `design.md` Decision 1, `tasks.md` 3.1/3.2.

**Problem:** The middleware sits outside `sessions`, so the response it gets already carries the headers that the session layer and the inner layers set. One of them is `Set-Cookie`, for example after `require_board_member` refreshed the token (`auth/mod.rs:136–140` writes the session). The design says the middleware "replaces the response". If it builds a new `Response`, these headers are lost.

**Fix:** Say that the middleware keeps the response parts (status and headers), replaces only the body, removes `Content-Length`/`Content-Type` and sets `text/html`, then adds `HX-Reswap` and `HX-Push-Url`. Add a 3.1 assertion that a header the inner handler set, such as `Set-Cookie`, is still in the toast answer.

### F3 (Medium): `detail.ctx` does not mean "the network failed"

**Files:** `design.md` Decision 3, `tasks.md` 4.1.

**Problem:** In htmx 4.0.2, `#issueRequest` catches everything inside its `try`, and that includes `await this.swap(ctx)`. It then fires `htmx:error` with `{ctx, error}` (l. 622). An exception during a swap therefore also has a `ctx`, for example a bad partial selector or an extension error. Such a client bug would show "Could not reach the server".

**Fix:** Check `detail.ctx && !detail.ctx.response`. `fetch` threw, or the request timed out, before a response arrived. `ctx.response` is set right after `fetch` (l. 590). Update 4.1's source test to look for the `response` check.

### F4 (Medium): the traffic page's existing error toast is not migrated

**Files:** `design.md` Decision 2, `tasks.md` 2.3, spec "Toast region on every page".

**Problem:**

- `egress/log/deny.rs:60` `toast_error` is a third toast markup: `role="alert"`, no frame and no Dismiss button. `deny.rs:47` sends it through `into_toast` with a 422 status. The requirement "Each toast SHALL have a 'Dismiss' button" is false for it after this change.
- `deny.rs:28` has a literal `hx-target="#allow-toast"`. Task 2.3 does not name it, so removing `#allow-toast` leaves a dangling target.
- Toasts also go out through two different mechanisms: `into_toast` with `HX-Retarget`/`HX-Reswap: innerHTML`, and the new partial with `HX-Reswap: none`.

**Fix:**

- In 2.3, replace `toast_error` with `components::error_toast` (or the `toast` frame).
- Change `deny.rs:28` (and its test in `egress/tests/deny_log.rs:160`).
- Let `into_toast` answer with `partial(Region::Toasts.selector(), "innerHTML", body)` plus `HX-Reswap: none`. Then there is one way to send a toast, and the `hx-select` concern of Decision 1 is covered for success toasts too.

### F5 (Medium): focus lands on an invisible element

**Files:** `design.md` Decision 7, `tasks.md` 6.2/6.3, spec "Focus follows the selected task or chat".

**Problem:** After a row is selected, focus moves to an `sr-only` `h2`. A sighted keyboard user then has no visible focus, and the next Tab continues from a point they cannot see (WCAG 2.4.7). The new region also sits next to the existing `#ws-details-heading` (the tabs heading, `sse.rs:316/341`), and the design does not give the new ID.

**Fix:** Focus a visible element, for example the head `header` (`#task-head`/`#chat-head`) with `tabindex="-1"`, `aria-labelledby` set to the sr-only heading, and the design system's focus ring. Or keep the heading and give it a visible focus style. Give the ID a distinct name, such as `detail-title`. Add a scenario (or extend 6.4) that checks the focus is visible.

### F6 (Medium): tests and docs sit in the final group 7

**Files:** `tasks.md` 7.1, 7.2.

**Problem:** The brief requires that no test or doc work is pushed to a final group. Task 7.1 (the user docs, `just docs-build`) and task 7.2 (`crates/df-web/AGENTS.md`) come after all the behaviour. The `apply` guidance in `openspec/config.yaml` says that docs are updated "in the same change as the behavior they describe".

**Fix:**

- Move 7.1 into group 3, after 3.3, where the error toast exists. Add the network-toast sentence in group 4.
- Split 7.2 into 1.3 (`Region`/`partial` rule), 3.4 (`error_page` rule) and 4.1/6.3 (plain scripts).
- Keep group 7 for the final gate (fmt, clippy, machete, tsc, `openspec validate`).

### F7 (Low): `row.closest('ul')` breaks with nested lists

**Files:** `design.md` Decisions 4 and 5, `tasks.md` 5.2.

**Problem:** `task-list-tree`, which is planned in parallel, lists subtasks "indented one level" below their parent (`task-list-tree/specs/task-ui/spec.md:47`). If those rows end up in a nested `ul`, `closest('ul')` clears `aria-current` on one level only, and two rows are marked.

**Fix:** Put a `data-rows` attribute on the list container (`#task-rows`/`#chat-rows`) and use `row.closest('[data-rows]')`. Or use `this.$root.querySelectorAll('[data-rows] [aria-current]')`.

### F8 (Low, simplicity): `Region` covers far more IDs than the problem needs

**Files:** `design.md` Decision 4, `tasks.md` 1.3.

**Problem:** The rule "every fixed ID that a selector names" takes in about 40 IDs: 44 distinct `"#…"` literals, plus the `hx-target` attributes. Most of them are local to one page and one file: `#form`, `#forge-hint`, `#save-status`, `#signing-key`, `#agent-fields`, `#archive-body` and others. One crate-wide enum couples every page, and task 1.3 touches almost every view, which collides with DF-38 and `task-list-tree`. The problem in "Why" is the IDs that "several files must keep the same".

**Fix:** Give `Region` only the IDs named in more than one file, or named by `sse.rs`, the middleware or a script. Page-local IDs stay literal next to their `id=`. Adjust the 1.3 grep check to match. This is a narrower version of the brief's item, not a rejection.

### F9 (Low): `selector()` returns `String`, and the unit test proves nothing

**Files:** `design.md` Decision 4, `tasks.md` 1.1.

**Problem:**

- `HX-Retarget` callers use `HeaderValue::from_static` (`config_pages.rs:322`, `project_subscriptions.rs:529`). A `String` selector forces a fallible `HeaderValue::from_str` there.
- With `format!("#{}", self.id())`, the test "`selector() == "#" + id()`" checks the formula against itself.
- `ALL` is maintained by hand: a variant missing from it escapes the uniqueness test.

**Fix:**

- Make it `const fn selector(self) -> &'static str` with literal match arms (or one small `macro_rules!` that writes both). Then the test is meaningful.
- Cover `ALL` with a test that matches exhaustively on each variant, for example a `match` that returns its index and is compared with `ALL.len()`.

### F10 (Low): design inaccuracies in Decision 1

**Files:** `design.md` Decision 1.

**Problem:**

- "`hx-select-oob` would still run" (the `HX-Retarget` alternative): `#processOOB` (l. 1111–1131) applies `hx-select-oob` to the response fragment. A toast body contains none of those IDs, so it does nothing with either approach. The `hx-select` argument alone carries the decision.
- "it also covers `csrf_guard`": `csrf_guard` calls `error_page` only for requests that are **not** htmx (`auth/mod.rs:115`), so the middleware never sees a marked htmx answer from it.
- `HX-Reswap: none` does not affect the main swap: a body with only partials skips it anyway (l. 1335–1340, which the Context already states). The header still matters against `hx-status:*` attributes and `delete` swaps, so keep it and say why.

**Fix:** Drop the `hx-select-oob` sentence and the `csrf_guard` mention. Add one line on why `HX-Reswap: none` stays.

### F11 (Low): error answers that bypass `error_page` still blank their target

**Files:** `design.md` Non-Goals and Risks.

**Problem:** htmx 4 swaps an empty body when the response has no partials (l. 1339). These answers come with an empty body and a target that is swapped:

- `egress/project_tab.rs:107`: `400`. The control has `hx-target=(REGION) hx-swap="outerHTML"`, so the whole `#project-egress` region disappears.
- `markdown_preview.rs:45`: `413`.
- `egress/log/mod.rs:95`: `503` on the stream request.

The mitigation in Risks (module docs plus a test of `not_found`/`internal_error`) does not catch them.

**Fix:** Convert the three to `error_page` in this change (one line each). Or list them under the backlog item for non-`error_page` answers.

### F12 (Low): task hygiene

**Files:** `tasks.md`.

**Problems and fixes:**

- 3.4, 6.1, 6.2 and 7.2 have no verification step. Add "verify that … passes", or `just docs-build` for doc tasks.
- 2.1, 5.1–5.3 and 6.1–6.3 describe the code before the test. Group 3 shows the right order ("Write the failing tests…"). Use it everywhere.
- The spec scenarios "Selecting a task fails" (URL unchanged) and "Failed selection keeps focus" have no browser test; only the headers are unit-tested. Extend 6.4: fulfil the row's GET with 500 through `page.route`, then check that the URL is unchanged, a toast is shown and focus is on the row.

### F13 (Nitpick): the out-of-scope targets for the roadmap

**Files:** `proposal.md` Out of Scope, `design.md` Risks.

**Problem:** "Component and token work: DF-38" names neither an iteration, the backlog nor a change in `product/iterations/roadmap.md` (no `DF-38` there). The `aria-busy` follow-up says "backlog, only if reported". Is it a real follow-up or rejected?

**Fix:** Name the roadmap target (or the change) for DF-38, and decide on `aria-busy`.

### F14 (Nitpick): nested live regions

**Files:** `design.md` Decision 2.

**Problem:** An error toast with `role="alert"` inside a `role="status" aria-live="polite"` container can be announced twice by some screen readers (unverified).

**Fix:** Add it to the screen-reader check of 2.2. If it is announced twice, keep the live attributes only on the container and drop `role="alert"`, or the other way round.

## Not covered

- No browser or screen-reader run. This is a planning review and nothing is implemented. The toast offset above the phone bottom bar stays unverified, as the design says.
- No `cargo` or `tsc` run: nothing to build. The claims about the code were checked by reading and grep.
- I did not check whether `tower-sessions` in this workspace sets `Set-Cookie` on every response or only on modified sessions. F2 holds either way, because at least the modified case loses its cookie.

## Next owner

Fixes to the artifacts: the author of the change (Linus Torvalds), in the order above. Nothing here needs a decision from Steve Wozniak, except F8 if the owner wants the full-crate `Region` as the brief said.
