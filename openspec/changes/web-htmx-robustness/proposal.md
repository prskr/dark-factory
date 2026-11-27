# Proposal

## Why

htmx 4 swaps every response status. When an action fails, the error answer is a full page (`auth::error_page`, used by `responses::not_found` and `responses::internal_error`), and htmx swaps that page into the small target of the action, for example `#pending`. Also: element IDs are bare strings that several files must keep the same, script code still sits in Rust strings although `crates/df-web/AGENTS.md` forbids that, and screen readers do not hear about live transcript items or a newly selected task.

## What Changes

- **Error answers for htmx requests.** An error answer to an htmx request (not a history restore) is a toast in the page's toast region. The original target stays as it was, and the URL does not change. Browser navigation still gets the full error page. The 422 answers (invalid forms) and 409 answers stay as they are.
- **Network errors.** When the server cannot be reached, a script shows a "could not reach the server" toast in the same region.
- **One toast region per page, one way to send a toast.** The layout renders the region on every page. The traffic page's `#allow-toast` is removed. Its allow and deny toasts, including the deny error toast, use the shared region, the shared toast frame and the same answer shape as error toasts.
- **Error answers without a body.** The empty 400 of the project egress tab (`egress/project_tab.rs`) blanks its region today. It becomes an `error_page` answer.
- **Typed region IDs.** A `Region` enum gives each element ID that more than one file names, or that live updates, the error middleware or a script names. Markup and live updates use the enum for `id=` and for the selector. IDs that only one page uses stay literal. One `partial()` helper replaces the three copies and `partial_str`.
- **Inline script code moves to TypeScript.** The composer's `x-init`/`x-on:submit` IIFEs and its concatenated `x-data` become a `composer` component. The transcript scroller's `x-init` observers become a `transcriptScroll` component. The task row's `x-on:click` becomes `select(row)` on `workspace` (as `chatLayout` already has). The body's `x-on:popstate` regex moves to a small component.
- **Accessibility after swaps.** `#transcript` gets `role="log"` and `aria-live="polite"`. After a board member selects a task or chat in the list, focus goes to the head of the detail, with a visible focus ring.
- **Rejected:** answering row navigation with only `#ws-detail`. The full render cut down by `hx-select` stays (see design, Decision 6).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `web-ui-shell`: new requirements for error toasts of failed actions, the network-error toast, one toast region per page, and focus after a task or chat is selected.
- `task-ui`: new requirements for the transcript as a live region and for the composer that remembers project and assignee (this behaviour exists, but no spec describes it yet).

## Impact

- `crates/df-web`: new module `htmx.rs` (`Region`, `partial`, `is_htmx`, the error-answer middleware), `auth/mod.rs` (`error_page` marks its response), `lib.rs` (middleware layer), `layout.rs` (toast region, network-error template, new scripts in `COMPONENT_SCRIPTS`, popstate component), `components` (error toast markup), `sse.rs`, `requests.rs`, `egress/log/{viewer,toast,page,allow,deny}.rs`, `egress/project_tab.rs`, `task_pages.rs` (transcript scroller), `task_pages/view.rs`, `chat_pages/view.rs`, `transcript/mod.rs` and the pages that name a shared region.
- New scripts in `crates/df-web/scripts/`: `composer.ts`, `transcriptScroll.ts`, `pageView.ts`, `htmxErrors.ts`, `focusAfterSwap.ts`. `workspace.ts` gets `select(row)`, and `chatLayout.ts`'s `select` stops using the `#chat-rows` ID.
- No new dependency, no database or protobuf change.
- Docs: `docs/content/using/tasks.md` and `docs/content/using/chats.md` get a short note on how failed actions are shown. `crates/df-web/AGENTS.md` gets the rules for regions, partials and error answers.
- Domain model unchanged: toasts and regions are UI terms, not domain terms. No ADR, because every decision here is easy to reverse.

## Out of Scope

- JS-free forms and changes to the CSRF guard: rejected (ADR-0011 stays).
- Component and token work: not a follow-up of this change. Task DF-38 plans it in parallel as its own change.
- Toasts for axum extractor rejections (plain-text 400/422 from `Form`/`Path`/`Query` parsing): backlog. They need a schema mismatch between the page and its handler, which tests catch.
- `aria-busy` on streaming transcript messages: rejected. Add it only if screen-reader users report that `role="log"` is too chatty.
