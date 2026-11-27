# Design

## Context

See proposal.md, "Why". Facts that shape the approach. The htmx facts were checked in `htmx.org@4.0.2` (`dist/htmx.js`, `dist/ext/hx-sse.js`), the version that `crates/df-web/package.json` pins:

- htmx 4 swaps every status except 204 and 304 (`config.noSwap`). It applies `HX-Retarget`, `HX-Reswap` and `HX-Reselect` before the swap. It processes `<hx-partial>` elements of a response regardless of the main swap style. When a response has partials and nothing else, the main swap is skipped. When `ctx.swap` is `none`, the document title is not changed.
- Even after an error status, htmx updates history for `hx-push-url="true"`. A response header `HX-Push-Url: false` turns this off (`#resolveHistoryAction`).
- `hx-select` filters the main swap content. With `HX-Retarget` alone, a toast answer to a row link (`hx-select="#ws-detail"`) would be filtered away.
- History restore (back/forward) fetches the URL with `HX-Request: true` and `HX-History-Restore-Request: true` and swaps the whole body.
- `htmx:error` fires with `{ctx, error}` when `fetch` throws. It does not fire for an `AbortError`, except on a timeout. It also fires with `{error}` only (no `ctx`) for script errors in `hx-on` handlers and during processing. A stream of `hx-sse` that drops fires `htmx:sse:error`, not `htmx:error`, and reconnects. The first request of a stream goes through the normal request path, so an error answer to it is swapped like any other.
- `htmx:after:swap` fires on `ctx.sourceElement` with `{ctx}`, also after a `none` swap.
- The answers that need the change come from `auth::error_page`. It is called directly and through `responses::not_found` and `responses::internal_error`: about 157 call sites in `df-web`, all without access to the request headers. `auth::is_htmx` already detects `HX-Request: true`.
- Today, three toast answers exist only on the traffic page (`egress/log/toast.rs`). They use `HX-Retarget: #allow-toast` and `HX-Reswap: innerHTML` into a `role="status"` region of that page.
- The task head and the chat head have no heading element. The list column has `h1` "Tasks" or "Chats".

## Goals / Non-Goals

**Goals:**

- No error answer to an htmx request replaces its target with a page.
- Region IDs and the selectors that name them come from one definition.
- No script code in Rust strings, as `crates/df-web/AGENTS.md` requires.

**Non-Goals:**

- A toast for every non-2xx answer. Only the answers of `error_page` change. Plain-text rejections of axum extractors stay as they are: backlog.
- Answering row navigation with `#ws-detail` only: rejected (Decision 6).
- Typed swap styles (`innerHTML`, `outerMorph`, …): rejected for now. The strings are local to the call and checked by tests.
- Dynamic per-item IDs (`#row-DF-12`, transcript item IDs, egress row IDs) as `Region` values: rejected. They are built from data and stay `format!`ed strings next to their `id=`.

## Decisions

### 1. Error answers: response marker plus one middleware

`error_page` keeps its signature. It renders the full page as today and also inserts an extension `ErrorAnswer { title, message }` into the response. A new middleware `htmx::error_answers` in `htmx.rs` reads `is_htmx(request)` and `HX-History-Restore-Request` before it calls `next`. If the request is an htmx request, is not a history restore, and the response carries `ErrorAnswer`, the middleware replaces the response with:

- the same status (htmx still fires `htmx:response:error`, and logs and tests see the real status);
- the body `partial(Region::Toasts.selector(), "innerHTML", error_toast(status, title, message))`;
- `HX-Reswap: none` (the original target is not touched) and `HX-Push-Url: false` (row links with `hx-push-url` do not change the URL).

The middleware is the outermost layer of the `ui` router in `lib.rs`, outside `sessions`, so it also covers `csrf_guard`, `require_board_member`, `require_admin` and the login routes. Responses without the marker, including SSE streams, pass through unchanged.

Alternatives:

- Pass `&HeaderMap` (or an `Htmx` extractor) into `not_found`/`internal_error`: this changes 157 call sites, and every new handler would have to remember to do it. Rejected.
- `HX-Retarget: #toasts` with `HX-Reswap: innerHTML` (the brief's wording, and what `egress/log/toast.rs` does): `hx-select` on the source element would filter the toast away (row links), and `hx-select-oob` would still run. A partial with `HX-Reswap: none` has neither problem. The brief's intent stays the same: the toast goes to the toast region and the original target is not touched.
- Answer 200 instead of the error status: this hides failures from `htmx:response:error` and from logs. Rejected.

The 422 (`responses::invalid`) and 409 answers do not use `error_page`, so they stay as they are.

### 2. One toast region, rendered by the layout

`layout()` renders `div id=(Region::Toasts.id()) role="status" aria-live="polite"` in a fixed corner: `bottom-6 right-6` from 768 px, and above the phone bottom bar below that (the exact offset is checked against the bottom bar's height in the browser during implementation, unverified). The error page itself also renders the region (it uses `layout`).

`components` gets `toast(kind, body)`, the frame that `egress/log/toast.rs::created` has today (panel, icon, `x-data="{ open: true }"`, Dismiss button). `error_toast(status, title, message)` uses this frame with an alert icon and `role="alert"` on the toast, so that screen readers announce errors at once. Success toasts rely on the region's `role="status"`. `egress/log/toast.rs` keeps its content (`created`, `highlight_href`), and `into_toast` targets `Region::Toasts`. The `#allow-toast` element on the traffic page is removed.

A new toast replaces the shown one (`innerHTML`). Toasts stay until the board member dismisses them or the next toast replaces them. There is no auto-dismiss, so that a slow reader still sees an error.

### 3. Network errors: `htmxErrors.ts` plus a server-rendered template

The layout renders `template id="network-error-toast"` that contains `error_toast(…)` with "Could not reach the server" / "Check your connection and try again." The new plain script `htmxErrors.ts` (not an Alpine component, like `sse-cleanup.ts`) listens on `document` for `htmx:error`. When `detail.ctx` is set (a request failed, not a script error), it clones the template into `#toasts`, and Alpine initialises the cloned `x-data`. The markup and wording stay in Rust. The script knows only the two IDs. It reads them from `data-toasts` and `data-network-error` on `<body>`, which the layout fills from `Region`, so the TypeScript has no ID literal.

Stream drops fire `htmx:sse:error`, so live updates reconnect without a toast. A failing first stream request after a page load does show the toast. This is correct: the page cannot update.

A global `htmx:response:error` handler is not needed: every error answer that matters already carries its toast (Decision 1).

### 4. `Region` and `partial()` in `htmx.rs`

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Region { Pending, Runs, TaskHead, ChatHead, WsDetail, Toasts, … }

impl Region {
    pub(crate) const ALL: &[Region] = &[…];
    pub(crate) const fn id(self) -> &'static str { match self { Region::Pending => "pending", … } }
    pub(crate) fn selector(self) -> String { format!("#{}", self.id()) }
}

pub(crate) fn partial(target: &str, swap: &str, inner: impl maud::Render) -> Markup
```

- A variant exists for each fixed element ID that a selector in Rust code names: `hx-target`, `hx-select`, `hx-select-oob`, `hx-include`, `<hx-partial hx-target>`, `HX-Retarget`, `data-*` attributes read by scripts. Markup uses `id=(Region::X.id())`, selectors use `Region::X.selector()`. IDs that only label an element (`for=`, `aria-labelledby`) or that nothing selects stay literal.
- `partial` takes `&str`, so dynamic per-item targets (`#{dom_id}`) keep working. `partial_str` callers pass `maud::PreEscaped(&graph)` and call `.into_string()`. The copies in `sse.rs`, `requests.rs` and `egress/log/viewer.rs` are removed.
- `is_htmx` moves from `auth` to `htmx.rs`. The header helpers `htmx_redirect`, `htmx_location` and `invalid` stay in `responses.rs` (they are about answers, not about the protocol).
- `egress/project_tab.rs`'s `REGION` constant becomes `Region::ProjectEgress`.
- Unit tests: `ALL` has unique IDs, and each `selector()` is `#` + `id()`.
- TypeScript does not share the enum. Scripts get IDs through `data-*` attributes rendered from `Region` (Decision 3, 5), or they navigate the DOM relative to their element (`select(row)` uses `row.closest('ul')`). Existing scripts that query a region ID are switched where this change touches them (`chatLayout.select`). Other scripts stay as they are.

Alternative: string constants (`const PENDING: &str = "#pending"`) next to each owner, as `project_tab.rs` does. They are scattered and do not give `id` and selector from one value. Rejected in favour of one enum, which shows every live region in one place.

### 5. Inline script code to TypeScript

- **`composer`** (`scripts/composer.ts`): `x-data="composer({ issue, branch })"`. The JSON is built with `serde_json` and escaped by maud, and replaces the concatenated object (`issue` keeps its digit filter on the server). `init()` reads `localStorage['df.composer']` in `try/catch` and preselects `$refs.project` / `$refs.persona` unless `data-explicit` is set or the value is not offered. `remember()` writes it and is bound by `x-on:submit="remember()"`. `linking` stays component state. The Ctrl/Meta+Enter bindings stay inline (short expressions).
- **Task row click**: `workspace.ts` gets `select(row)`, the same as `chatLayout.select`. Both use `row.closest('ul')` instead of `#task-rows` / `#chat-rows`. The row becomes `x-on:click="select($el)"`.
- **Popstate**: the body's `x-data=""` and its regex become `x-data="pageView"` (`scripts/pageView.ts`) with `syncView()` bound by `x-on:popstate.window="syncView()"`. The rendering condition (workspace pages only) stays as it is.
- The three files go into `COMPONENT_SCRIPTS`. Tests read their source with `assets::script_source` (as for the existing components), and the page tests assert the new attributes.

### 6. Row navigation keeps the full render (item 5 of the brief: rejected)

Answering `GET /tasks/DF-11` with only `#ws-detail` when `HX-Request` is set would save the list query and its rendering. But the handler would also have to tell history restores (`HX-History-Restore-Request`, a full body swap) and `hx-select` answers apart, put the `<title>` into the fragment, and keep both paths tested in `task_pages` and `chat_pages`. The list query is one indexed query. The gain does not pay for a second rendering path. Rejected; it can come back with numbers that show a slow list.

### 7. Accessibility after swaps

- `#transcript` gets `role="log"` and `aria-live="polite"` (`transcript::render_transcript`). The live update appends items into it, so they are announced. Morphs of a streaming message also announce text that changed. This is acceptable for a log, and `aria-atomic` stays false.
- The task head and the chat head get a first child `h2 id=(Region::DetailHeading.id()) tabindex="-1" class="sr-only"` with key and title. It is `h2` because the list column owns the page's `h1`. It is screen-reader only because the visible title of the task head is inside the rename button, whose `aria-label` hides the text, and the chat head shows the title only on phones.
- Row links get `data-focus-after-swap=(Region::DetailHeading.selector())`. The plain script `focusAfterSwap.ts` listens for `htmx:after:swap` on `document`. If the source element has `data-focus-after-swap` and `ctx.response.status < 400`, it focuses that selector. Error toasts (status ≥ 400) and SSE swaps (no such attribute) leave focus alone. The attribute is generic, so other places can use it later.

## Risks / Trade-offs

- [A handler builds its own error answer without `error_page`] → it still swaps a page or text into the target. Mitigation: the module docs of `responses.rs` say that error answers go through `error_page`. A test in `htmx.rs` covers `not_found` and `internal_error`.
- [The middleware sees every UI response] → It only reads one request header and one response extension. Bodies are not buffered, so SSE is not affected. A test sends an SSE request with `HX-Request` and checks that the stream content type passes through.
- [`role="log"` on a busy transcript is chatty with screen readers] → `polite` waits for idle. If users find it too much, streaming chunks can later be marked `aria-busy` while they stream (backlog, only if reported).
- [A sr-only heading duplicates the visible title for screen readers on phones] → It is a minor repetition, and it gives one reliable focus target on all widths.
- [Other open changes (DF-38 components, `task-list-tree`) touch the same files] → `Region` and `partial` are mechanical replacements. Whichever change lands second rebases them.

## Migration Plan

No data, configuration or API change. Deploy as usual; roll back by reverting.
