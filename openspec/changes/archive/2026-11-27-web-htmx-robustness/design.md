# Design

## Context

See proposal.md, "Why". These facts shape the approach. The htmx facts were checked in `htmx.org@4.0.2` (`dist/htmx.js`, `dist/ext/hx-sse.js`), the version that `crates/df-web/package.json` pins, and confirmed by the review (`review.md`):

- htmx 4 swaps every status except 204 and 304 (`config.noSwap`). It applies `HX-Retarget`, `HX-Reswap` and `HX-Reselect` before the swap. It processes `<hx-partial>` elements of a response whatever the main swap style is. When a response has partials and nothing else, the main swap is skipped. When `ctx.swap` is `none`, the document title is not changed. A body that is empty and has no partials is swapped like any other body, so the target is emptied.
- Even after an error status, htmx updates history for `hx-push-url="true"`. A response header `HX-Push-Url: false` turns this off (`#resolveHistoryAction`).
- `hx-select` filters the main swap content. With `HX-Retarget` alone, a toast answer to a row link (`hx-select="#ws-detail"`) would be filtered away.
- History restore (back/forward) fetches the URL with `HX-Request: true` and `HX-History-Restore-Request: true` and swaps the whole body.
- `htmx:error` fires with `{ctx, error}` for anything that throws in `#issueRequest`: a failed `fetch`, a timeout, and also an exception during the swap. It does not fire for an `AbortError`, except on a timeout. `ctx.response` is set right after `fetch` returns, so a missing `ctx.response` means that no answer arrived. `htmx:error` also fires with `{error}` only (no `ctx`) for script errors in `hx-on` handlers and during processing. A stream of `hx-sse` that drops fires `htmx:sse:error`, not `htmx:error`, and reconnects. The first request of a stream goes through the normal request path.
- `htmx:after:swap` fires on `ctx.sourceElement` with `{ctx}`, also after a `none` swap.
- The answers that need the change come from `auth::error_page`. It is called directly and through `responses::not_found` and `responses::internal_error`: 158 call sites in `df-web`, all without access to the request headers. `auth::is_htmx` already detects `HX-Request: true`.
- The traffic page has three toast answers (`egress/log/toast.rs`, `egress/log/deny.rs`). They use `HX-Retarget: #allow-toast` and `HX-Reswap: innerHTML` into a `role="status"` region of that page. The deny error toast (`deny.rs` `toast_error`) has no frame and no Dismiss button.
- Three error answers have no body: `egress/project_tab.rs` (400, its control targets `#project-egress` with `outerHTML`, so the region disappears), `markdown_preview.rs` (413, requested with `fetch` by `markdownField.ts`, which handles it) and `egress/log/mod.rs` (503 on the stream request, whose element has `hx-swap="none"`). Only the first one harms the page.
- The task head and the chat head have no heading element. The list column has `h1` "Tasks" or "Chats". The visible title of the task head is inside the rename button, whose `aria-label` hides the text. The chat head shows the title only below 768 px.
- Script code in Rust strings today: the composer's `x-init`/`x-on:submit` IIFEs (`task_pages/view.rs`), the task row's `x-on:click` (`task_pages/view.rs`), the body's `x-on:popstate` regex (`layout.rs`), and the transcript scroller's `x-init` with a `MutationObserver` and two `ResizeObserver`s (`task_pages.rs::transcript_scroll`). `SEND_SHORTCUT` (`task_pages/view.rs`) is one short expression with one `querySelector`.

## Goals / Non-Goals

**Goals:**

- No error answer to an htmx request replaces its target with a page or with nothing.
- Element IDs that several places must keep the same come from one definition.
- No script code in Rust strings, as `crates/df-web/AGENTS.md` requires. Short inline expressions stay (`$nextTick(() => …)`, `SEND_SHORTCUT`).

**Non-Goals:**

- A toast for every non-2xx answer. Only `error_page` answers change, and `egress/project_tab.rs`'s empty 400 is converted to one. Plain-text rejections of axum extractors stay as they are: backlog. The 413 of the Markdown preview and the 503 of the traffic stream stay, because their callers do not swap them (see Context).
- Answering row navigation with `#ws-detail` only: rejected (Decision 6).
- Typed swap styles (`innerHTML`, `outerMorph`, …): rejected for now. The strings are local to the call and checked by tests.
- `Region` values for IDs that only one page uses, and for dynamic per-item IDs (`#row-DF-12`, transcript item IDs, egress row IDs): rejected. They stay literal or `format!`ed next to their `id=`.
- `aria-busy` while a transcript message streams: rejected, unless screen-reader users report that the log is too chatty.

## Decisions

### 1. Error answers: response marker plus one middleware

`error_page` keeps its signature. It renders the full page as today and also inserts an extension `ErrorAnswer { title, message }` into the response. A new middleware `htmx::error_answers` in `htmx.rs` reads `is_htmx(request)` and `HX-History-Restore-Request` before it calls `next`. If the request is an htmx request, is not a history restore, and the response carries `ErrorAnswer`, the middleware changes the response in place:

- it keeps the status and every header of the inner response (for example a `Set-Cookie` written by the session layer after a token refresh);
- it removes `Content-Length`, sets `Content-Type: text/html; charset=utf-8` and replaces only the body with `partial(Region::Toasts.selector(), "innerHTML", error_toast("<status> <title>", message))`;
- it adds `HX-Reswap: none` and `HX-Push-Url: false`. A body with only partials already skips the main swap. `HX-Reswap: none` still stops `hx-status:*` attributes and `delete` swaps of the source element from acting on the error. `HX-Push-Url: false` stops row links with `hx-push-url` from changing the URL.

The status stays, so htmx still fires `htmx:response:error`, and logs and tests see the real status.

The middleware is the outermost layer of the `ui` router in `lib.rs`, outside `sessions`, so it covers `require_board_member`, `require_admin`, the login routes and all pages. Responses without the marker, including SSE streams, pass through unchanged, and no body is buffered.

Alternatives:

- Pass `&HeaderMap` (or an `Htmx` extractor) into `not_found`/`internal_error`: this changes 158 call sites, and every new handler would have to remember to do it. Rejected.
- `HX-Retarget: #toasts` with `HX-Reswap: innerHTML` (the brief's wording, and what `egress/log/toast.rs` does today): `hx-select` on the source element would filter the toast away (row links). The partial does not depend on the source element's attributes. The brief's intent stays the same: the toast goes to the toast region and the original target is not touched.
- Answer 200 instead of the error status: this hides failures from `htmx:response:error` and from logs. Rejected.

The 422 (`responses::invalid`) and 409 answers do not use `error_page`, so they stay as they are. `egress/project_tab.rs`'s empty 400 becomes `error_page(StatusCode::BAD_REQUEST, "Bad request", "The port is not valid.")`.

### 2. One toast region, one toast frame, one toast answer

`layout()` renders `div id=(Region::Toasts.id()) role="status" aria-live="polite"` in a fixed corner: `bottom-6 right-6` from 768 px, and above the phone bottom bar below that (the offset is checked against the bottom bar's height in the browser during implementation, unverified). The error page itself also renders the region (it uses `layout`).

`components` gets:

- `toast(tone, body)`: the frame that `egress/log/toast.rs::created` has today (panel, icon by tone, `x-data="{ open: true }"`, Dismiss button);
- `error_toast(title, message)`: the frame with the error tone and an alert icon;
- `toast_answer(status, body) -> Response`: `partial(Region::Toasts.selector(), "innerHTML", body)` with `HX-Reswap: none`. It is the one way to send a toast. `egress/log/toast.rs::into_toast` is replaced by it, and the middleware uses the same partial and headers.

The toasts carry no live role of their own. The region's `role="status"` announces both success and error toasts. This avoids nested live regions, which some screen readers announce twice. An error that waits for a polite announcement is acceptable, because the region the user acted on did not change. The screen-reader check in task 2.2 confirms one announcement.

`egress/log/toast.rs` keeps its content (`created`, `highlight_href`). `deny.rs::toast_error` is replaced by `error_toast("Rule not created", message)`. The deny button loses `hx-target="#allow-toast"` and gets `hx-swap="none"`. The `#allow-toast` element on the traffic page is removed.

A new toast replaces the shown one (`innerHTML`). Toasts stay until the board member dismisses them or the next toast replaces them. There is no auto-dismiss, so that a slow reader still sees an error.

### 3. Network errors: `htmxErrors.ts` plus a server-rendered template

The layout renders `template id=(Region::NetworkErrorToast.id())` that contains `error_toast("Could not reach the server", "Check your connection and try again.")`. The new plain script `htmxErrors.ts` (not an Alpine component, like `sse-cleanup.ts`) listens on `document` for `htmx:error`. It acts only when `detail.ctx` is set and `detail.ctx.response` is not: the request got no answer (network failure or timeout). An exception during a swap has a response and shows no network toast, so a client bug does not look like an offline server. The script clones the template into the toast region, and Alpine initialises the cloned `x-data`. The markup and wording stay in Rust. The script reads both IDs from `data-toasts` and `data-network-error` on `<body>`, which the layout fills from `Region`, so the TypeScript has no ID literal.

Stream drops fire `htmx:sse:error`, so live updates reconnect without a toast. A failing first stream request after a page load does show the toast. This is correct: the page cannot update.

A global `htmx:response:error` handler is not needed: every error answer that matters already carries its toast (Decision 1).

### 4. `Region` and `partial()` in `htmx.rs`

One `macro_rules!` declares each region once, with its literal ID. From that it generates the enum, `ALL`, and the two `const fn`s, so `ALL` cannot miss a variant and the selector is a `&'static str` (usable in `HeaderValue::from_static`):

```rust
regions! {
    Pending => "pending",
    TaskHead => "task-head",
    Toasts => "toasts",
    // …
}
// expands to:
pub(crate) enum Region { Pending, TaskHead, Toasts, … }
impl Region {
    pub(crate) const ALL: &[Region] = &[Region::Pending, …];
    pub(crate) const fn id(self) -> &'static str { match self { Region::Pending => "pending", … } }
    pub(crate) const fn selector(self) -> &'static str { match self { Region::Pending => concat!("#", "pending"), … } }
}

pub(crate) fn partial(target: &str, swap: &str, inner: impl maud::Render) -> Markup
```

- **Which IDs are regions:** an element ID is a `Region` when a selector names it in a different file from its `id=`, or when `sse.rs`, the error middleware or a script (through a `data-*` attribute) names it. Examples are `#pending`, `#runs`, `#task-head`, `#chat-head`, `#task-details`, `#ws-detail`, `#ws-details-heading`, `#transcript`, `#composer-action`, `#composer-slot`, `#config-list`, `#project-egress`, the traffic counters and `#toasts`. IDs that only one file sets and selects (`#form`, `#forge-hint`, `#save-status`, `#signing-key`, `#agent-fields`, `#archive-body`, …) stay literal. This keeps the change out of most page files, which DF-38 and `task-list-tree` also touch.
- Markup uses `id=(Region::X.id())`, and selectors use `Region::X.selector()`. The constant `SEND_SHORTCUT` becomes a function `send_shortcut() -> String` that builds the expression with `format!` from `Region::ComposerAction.selector()`.
- `partial` takes `&str`, so dynamic per-item targets (`#{dom_id}`) keep working. `partial_str` callers pass `maud::PreEscaped(&graph)` and call `.into_string()`. The copies in `sse.rs`, `requests.rs` and `egress/log/viewer.rs` are removed.
- `is_htmx` moves from `auth` to `htmx.rs`. The header helpers `htmx_redirect`, `htmx_location` and `invalid` stay in `responses.rs`.
- `egress/project_tab.rs`'s `REGION` constant becomes `Region::ProjectEgress`.
- Unit tests: the IDs in `ALL` are unique, and each is a valid HTML ID (not empty, no whitespace, no `#`). `ALL` is complete by construction.
- TypeScript does not share the enum. Scripts get IDs through `data-*` attributes rendered from `Region` (Decisions 3 and 7), or they navigate relative to their element (Decision 5).

Alternative: a crate-wide `Region` for every ID that any selector names (about 40, the first version of this design). It would touch almost every view and couple unrelated pages. Rejected after review (F8). Another alternative is string constants next to each owner, as `project_tab.rs` does: they do not give the ID and the selector from one value. Rejected.

### 5. Inline script code to TypeScript

- **`composer`** (`scripts/composer.ts`): `x-data="composer({ issue, branch })"`. The JSON is built with `serde_json` and escaped by maud, and replaces the concatenated object (`issue` keeps its digit filter on the server). `init()` reads `localStorage['df.composer']` in `try/catch` and preselects `$refs.project` / `$refs.persona` unless `data-explicit` is set or the value is not offered. `remember()` writes it and is bound by `x-on:submit="remember()"`. `linking` stays component state. The Ctrl/Meta+Enter bindings stay inline (short expressions).
- **`transcriptScroll`** (`scripts/transcriptScroll.ts`): `x-data="transcriptScroll"` on `#transcript-scroll`, with `pinned` state, `init()` that sets up the `MutationObserver` and the two `ResizeObserver`s (the second on `this.$el.firstElementChild`, the transcript, so no ID literal), `destroy()` that disconnects them, and `onScroll()` bound by `x-on:scroll`. The 48 px threshold is a named field.
- **Task row click**: `workspace.ts` gets `select(row)`, the same as `chatLayout.select`. The list containers (`#task-rows`, `#chat-rows`) get `data-rows`. Both `select` methods clear `aria-current` with `row.closest('[data-rows]')?.querySelectorAll('[aria-current]')`. This works with the nested subtask rows of `task-list-tree` too. The row becomes `x-on:click="select($el)"`.
- **Popstate**: the body's `x-data=""` and its regex become `x-data="pageView"` (`scripts/pageView.ts`) with `syncView()` bound by `x-on:popstate.window="syncView()"`. The rendering condition (workspace pages only) stays as it is.
- `SEND_SHORTCUT` stays inline. It is one short expression, and moving it would need a component for the message form.
- The four files go into `COMPONENT_SCRIPTS`. Tests read their source with `assets::script_source`, and the page tests assert the new attributes.

### 6. Row navigation keeps the full render (item 5 of the brief: rejected)

Answering `GET /tasks/DF-11` with only `#ws-detail` when `HX-Request` is set would save the list query and its rendering. But the handler would also have to tell history restores (`HX-History-Restore-Request`, a full body swap) and `hx-select` answers apart, put the `<title>` into the fragment, and keep both paths tested in `task_pages` and `chat_pages`. The list query is one indexed query. The gain does not pay for a second rendering path. Rejected; it can come back with numbers that show a slow list.

### 7. Accessibility after swaps

- `#transcript` gets `role="log"` and `aria-live="polite"` (`transcript::render_transcript`). The live update appends items into it, so they are announced. Morphs of a streaming message also announce text that changed. This is acceptable for a log, and `aria-atomic` stays false.
- **Focus target:** the head itself (`#task-head` / `#chat-head`, both `Region`s already) gets `tabindex="-1"`, `aria-labelledby="detail-title"` and the design system's focus ring on `focus-visible`. Its first child is `h2 id="detail-title" class="sr-only"` with key and title. It is `h2` because the list column owns the page's `h1`. It is screen-reader only because the visible titles cannot carry the name (rename button, phone-only chat title). The ID is page-local, so it stays literal. Focusing a visible element gives sighted keyboard users a visible focus position (WCAG 2.4.7). Browsers show the `focus-visible` ring for programmatic focus after a keyboard action, not after a mouse click.
- Task row links get `data-focus-after-swap=(Region::TaskHead.selector())`, and chat row links get `Region::ChatHead`. The plain script `focusAfterSwap.ts` listens for `htmx:after:swap` on `document`. If the source element has `data-focus-after-swap` and `ctx.response.status < 400`, it focuses that selector. Error toasts (status ≥ 400) and SSE swaps (no such attribute) leave focus alone. The attribute is generic, so other places can use it later.

## Risks / Trade-offs

- [A handler builds its own error answer without `error_page`] → It swaps a page, text or nothing into the target. Mitigation: the known case (`project_tab.rs`) is converted. The module docs of `responses.rs` and `crates/df-web/AGENTS.md` say that error answers go through `error_page`. A test in `htmx.rs` covers `not_found` and `internal_error`.
- [The middleware sees every UI response] → It only reads two request headers and one response extension. Bodies are not buffered, so SSE is not affected. A test sends an SSE request with `HX-Request` and checks that the stream passes through.
- [`role="log"` on a busy transcript is chatty with screen readers] → `polite` waits for idle. `aria-busy` is rejected unless users report it (Non-Goals).
- [The sr-only heading repeats the visible title for screen readers on phones] → It is a minor repetition, and it gives one reliable accessible name on all widths.
- [Polite announcement of errors] → An error is announced after the current speech, not at once. The page did not change, so nothing is lost. If the screen-reader check finds it unclear, `role="alert"` moves to the error toast and the region loses `aria-live` (one live mechanism either way).
- [Other open changes (DF-38 components, `task-list-tree`) touch the same files] → Narrowing `Region` to shared IDs keeps most page files out. `select(row)` uses `data-rows`, which works with nested rows. Whichever change lands second rebases.

## Migration Plan

No data, configuration or API change. Deploy as usual; roll back by reverting.
