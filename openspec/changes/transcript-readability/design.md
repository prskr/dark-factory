# Design

## Context

- The transcript is rendered server-side with `maud` (`df-web/src/components/transcript.rs`). It is folded from stored proto events by `transcript::Folder` (`df-web/src/transcript/mod.rs`) and streamed over SSE as htmx partials (`df-web/src/sse.rs`):
  - a new item is sent as `#transcript beforeend`;
  - a changed item is sent as `#<dom_id> outerMorph`.
- htmx ignores `open` when it morphs, so open `<details>` elements stay open (`layout.rs`).
- Consecutive tool rows and notice rows share divider lines through CSS only (`styles/input.css`, `[data-tool],[data-system]`).
- Request cards (questions, approvals) are not folder items. They are merged in by time (`render_transcript`, `card_goes_before`) and inserted live `beforebegin` the next item.
- Markdown: `comrak` plus `syntect` (`df-web/src/markdown.rs`), with `highlight_by_extension` already available.
- Webhook wake-ups: `df-db::webhooks::queue_event_tx` queues a visible text block authored by `system(webhook)`. The runtime echoes it back as a `UserMessage`. Persisting that `UserMessage` marks the message delivered (`transcript-storage` "Delivery confirmation").
- Forge activity events are control events with a source key, so each is written once (`git_pushes.rs`, `TranscriptWriter::append_control`).
- The chat UI (`iter3-chats`) uses the same transcript component.

## Goals / Non-Goals

**Goals:**
- One grouping model, used by both the full render and the SSE stream, so a reload shows what the live view showed.
- No per-item re-render of large groups during live updates.
- No new dependencies.

**Non-Goals:**
- Client-side grouping in TypeScript. Rejected, see D3.
- Persisting a group's open state across reloads. Rejected: groups open collapsed, like tool rows.
- Holding egress connections while a `network-access` request is pending. Backlog in `product/iterations/roadmap.md`, see the proposal.

## Decisions

### D1 — UI agreed in PenPot before implementation

The thinking row, the tool group (collapsed, open, running, with failures), the webhook row variants, and the tool details (shell line, JSON, Markdown output, diff) are drawn in PenPot in light, dark and phone width. Implementation starts after the user approves them. Agreed details that change a spec scenario (labels, icons) go back into the specs first. Icons are lucide (`brain`, `webhook`) to match `components/icons.rs`. The group row reuses the wrench icon unless the design says otherwise.

### D2 — Webhook rows: hidden text plus a stored structured event, written on delivery

- **Queueing.** `queue_event_tx` stores the text as `text_block(render_message(event), true)` (hidden). It stores the event's display fields as an encoded proto `WebhookReceived` in a new nullable column `task_messages.forge_event BYTEA`.
- **Delivery.** When the control plane persists the `UserMessage` that confirms delivery of a message with `forge_event`, it appends a `ForgeActivity.webhook_received` control event with source key `webhook:<prompt_id>` in the same transaction. The source key makes redelivery idempotent. Reading only the column means the runtime cannot forge the event (spec "Webhook activity comes from the control plane only").
- **Proto sketch.** It mirrors `df_core::webhooks::WebhookEvent` and holds no payload text:
  ```proto
  message WebhookReceived {
    oneof kind {
      ReviewSubmitted review_submitted = 1;   // number, actor, verdict, url
      CommentCreated comment_created = 2;     // number, is_pull_request, actor, url
      PullRequestClosed pull_request_closed = 3; // number, actor, url (not merged)
      CiFailed ci_failed = 4;                 // head, check, url
    }
  }
  ```
  The folder adds `ForgeItem::Webhook`, and `render_forge` draws it with `Icon::Webhook`. The one-line text is a single function that the transcript row and the queued stack share (spec "Webhook messages in the queued stack").
- **Why on delivery and not at queue time:** a transcript event needs a run (`RunTaskLock`). A webhook often arrives while no run is active. Rows without a run are a separate backlog item.
- **Alternatives:**
  - Render the existing `system(webhook)` bubble as a row in the folder, with no data change. This was offered and not chosen: the user picked the structured event.
  - Write the event at queue time. Not possible without rows outside runs.

### D3 — Tool call groups are computed in the folder; the wrapper is created with the first tool call

- **Membership.** The folder assigns each item a `group: Option<GroupId>`:
  - a tool item opens a group, or joins the open one;
  - a thought or notice joins an open group;
  - any other item closes the group.
  - A thought before the first tool call of a streak stays outside the group. A trailing thought or notice stays in the group, because whether more tool calls follow is unknown when it arrives.
- **Markup.** A streak is rendered as a wrapper `div id="grp-<first dom_id>" data-tool-group` with three parts:
  1. A head (`#grp-…-head`): a button with `aria-expanded`, the count, the failed count and the running indicator.
  2. An items container (`#grp-…-items`).
  3. Client-only open state (Alpine `x-data` on the wrapper).
  The wrapper exists from the first tool call. With one tool call the head is hidden and the items always show, so it looks like a plain row today. A second call only morphs the head, which makes the 1 → 2 transition a head update and nothing else.
- **SSE.**
  - A new member is sent as `#grp-…-items beforeend`.
  - A changed member is sent as `#<dom_id> outerMorph`, as today.
  - The head is sent as `#grp-…-head outerMorph` whenever count, failed count or running state changes.
  - The wrapper itself is never morphed, so the Alpine open state and the `open` state of nested `<details>` survive.
- **Request cards.** Cards do not split a group. A card whose time falls inside a streak is placed after the group, both on load and live, because live tool calls keep appending into the group above the card. Spec: task-ui "Tool call groups", scenario "Approval request during a streak".
- **Alternatives:**
  - A TypeScript component that regroups DOM siblings. Rejected: counts and failure state would be derived twice, and a reload could differ from the live view.
  - Re-rendering the whole group on every member update. Rejected: streaks of hundreds of calls would be resent on every streamed tool update.

### D4 — Thinking rows reuse the tool row layout

`c::thought` becomes a `<details data-tool>`, so it shares the divider lines. It shows `Icon::Brain`, "Thinking", the first non-empty line truncated, and the chevron. The body is plain text while the turn runs. When `complete_agent_messages` runs it is switched to `markdown::render`, as agent messages are.

### D5 — Tool details through the existing renderer

`render_tool` builds typed blocks instead of `(label, String)`:

| Block | Rendering |
|---|---|
| Input | `serde_json` parse. If there is a top-level `command` string, `highlight_by_extension("sh", command)` comes first. Then `to_string_pretty` → `highlight_by_extension("json", …)`. On a parse error, plain escaped text. |
| Output text | `markdown::render` once the status is `completed` or `failed`; plain text while running (`markdown-rendering` "Highlighting only completed Markdown"). |
| Diff | `diff_text` → `highlight_by_extension("diff", …)`. |
| Result | Same as Input, without the command line. |

The size limit of `markdown-rendering` (no highlighting above 64 KiB) applies unchanged. Output truncation stays as stored.

### D6 — Scroll bug: reproduce first, then fix at the cause

The cause is not verified. Candidates:
- the document (not `#transcript-scroll`) overflowing, because `.ws` is `h-dvh` inside `min-h-dvh` while the phone variant uses `calc(100dvh-4rem)`;
- the browser scrolling the document to reveal the card's form controls;
- the `bg-black/40` scrim.

The first task is an e2e test (Playwright, skill `e2e`) that opens a task with a pending question at desktop and phone width. It asserts that `document.scrollingElement.scrollTop == 0`, that the card's bottom edge is inside the viewport, and that `#transcript-scroll` has no blank space after the last child. The fix is chosen from what the test shows and recorded here.

### D7 — GitHub Markdown extensions in the one options function

Tables show as raw text because `options()` (`markdown.rs`) starts from `comrak::Options::default()`, where every extension is off. D7 sets `extension.table`, `strikethrough`, `tasklist` and `autolink` there. Every renderer variant (`render`, `render_with_hard_breaks`, `render_with_images`) takes its options from `options()`, so the transcript, request cards, the inbox and artifacts get the same settings and cannot diverge.

- Autolinks become ordinary link nodes, so they pass the existing link handling: unsafe schemes are dropped, and `rel="noopener noreferrer nofollow"` is added.
- Task list items are checkboxes that comrak emits itself (`disabled`). `render.escape` still escapes raw HTML from the source.
- No PenPot step: `.md-body table` is already styled in `styles/input.css`.

## Risks / Trade-offs

- [Forgetting the group in SSE paths, so the live view differs from a reload] → A folder-level test that compares the grouping of a fold-all with the grouping of an event-by-event fold. A `df-web` SSE test covers the 1 → 2 → N tool transition and a card inside a streak.
- [Markdown rendering of large tool outputs on every page load] → Rendering happens once per finished item in the folder, the same cost as agent messages today. The highlight limit caps the expensive part.
- [Old webhook messages keep their bubble] → Accepted, pre-MVP.
- [An older control plane after a rollback does not know `WebhookReceived`] → prost decodes it as a `ForgeActivity` with `kind` unset, and `forge_item` returns no row, so the event is silently absent from the transcript. Acceptable: the row is informational, the webhook's effect (the agent's turn) stays visible, and there are no production installations.

## Migration Plan

- One `df-db` migration adds `task_messages.forge_event BYTEA NULL`. Then regenerate `.sqlx/`.
- The proto change is additive (`ForgeActivity` oneof variant 4).
- Rollback: drop the column in a follow-up migration. Stored `webhook_received` events are omitted from the transcript (see Risks).

## Open Questions

- Does every webhook kind carry a usable URL? If a review or comment event has none, the row links to the task's PR URL, or shows no link. This can be decided when the proto is written.
