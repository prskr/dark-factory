# Proposal

## Why

Coding runs make hundreds of tool calls in a row, and those rows push the agent's actual messages and the human's instructions out of view. Webhook wake-ups look like a human typed them. Thinking blocks look different from every other row. Tool output arrives as Markdown fences but is shown as raw text. Markdown tables are shown as raw text in the transcript, request cards and artifacts. A pending question at the end of the transcript scrolls the page into an empty, dark area. Together these make transcripts in the task UI and the chat UI hard to follow.

## What Changes

- **Webhook wake-ups as forge activity rows.** The wake-up text the agent receives becomes a hidden content block. The control plane stores the event's structured fields with the queued message. When the message is delivered, the control plane appends a new forge activity event `WebhookReceived` (kind, actor, PR/issue number, verdict or check name, URL). The transcript shows it as one row with a webhook icon, in the layout of the push and PR rows. The queued message stack shows the same one-line summary while the message waits. Webhook messages stored before this change keep their bubble (pre-MVP, no migration of old events).
- **Thinking rows like tool rows.** A thinking block becomes a collapsible row with a brain icon, a "Thinking" label, the first line as a truncated preview and a chevron. Once its turn has ended, the body is rendered as Markdown.
- **Tool call groups.** Two or more consecutive tool calls collapse into one group row: a count of tool calls, the number that failed, and a running indicator while one is in progress. Thinking blocks and notices between them stay inside the group. Agent text, user messages, forge and artifact rows and run dividers end a group. A request card raised during a streak does not end it; the card appears after the group. Expanding the group shows the nested tool rows, each expandable as today. Groups update live.
- **Readable tool input and output.** Tool output text goes through the existing safe Markdown renderer once the call has finished, so ```` ```console ```` and other fences are highlighted. Input JSON is pretty-printed and highlighted as JSON. A top-level `command` string is shown first as a highlighted shell line. Diffs are highlighted as diffs. Raw results are pretty-printed JSON.
- **GitHub Markdown extensions.** The shared renderer enables tables, strikethrough, task lists and autolinks. This applies everywhere it is used: the transcript, request cards, the inbox and artifacts. The existing safety rules apply unchanged; bare URLs that become links get the same scheme check and `rel` attribute.
- **Pending question scroll fix.** A transcript ending in a pending question or approval card opens with the card in view and no blank area below it. Neither the document nor the transcript scrolls past the last item.
- **Design first.** The UI parts (thinking row, tool group, webhook row, tool I/O) are drawn in PenPot and agreed on before implementation.
- **BREAKING (stored data shape):** new transcript event variant `ForgeActivity.webhook_received` and a new nullable column on `task_messages`. Older readers omit the new event: they decode it as a forge activity with no kind, which yields no row. There are no production installations.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `task-ui`: "Live transcript" (tool details highlighted instead of plain escaped text), "Forge activity as transcript rows" (webhook row), "Transcript follows the latest item" (pending card at the end). New requirements: thinking rows, tool call groups, readable tool input and output, webhook summaries in the queued stack.
- `forge-webhooks`: "Wake-up message per event" and "Message content". The wake-up text becomes a hidden block, and the message carries the structured event.
- `markdown-rendering`: a new requirement, "GitHub Markdown extensions".
- `forge-activity`: a new requirement, "Webhook events become forge activity", written once, on delivery, by the control plane only.

## Impact

- `contract/proto/darkfactory/transcript/v1/transcript.proto`: `ForgeActivity` gets the `WebhookReceived` variant.
- `df-db`: a migration for `task_messages.forge_event`, `webhooks::queue_event_tx`, the delivery-confirmation path that persists `UserMessage`, and regenerated `.sqlx/` metadata.
- `df-core`: `webhooks::render_message` stays as the agent's text. A mapping from `WebhookEvent` to the proto is added.
- `df-web`: the transcript folder (grouping), `components/transcript.rs` (thinking, group and webhook rows, tool I/O), `components/icons.rs` (lucide `brain`, `webhook`, `list-collapse` or similar), `markdown.rs` reused for tool output and JSON, with the extensions enabled in `options()` (no new styles: `.md-body table` is already styled), the SSE partials for groups, the scroll container in `task_pages.rs`, and `styles/input.css`.
- Chat UI (`iter3-chats`) shares the transcript component and gets the same behavior.
- No new dependencies: comrak and syntect are already in the tree.
- Docs: `docs/content/` task page and transcript reference pages.

## Out of Scope

- **Holding denied gateway connections while the user decides on a `network-access` request** (instead of a 403 right away). This is the separate change `egress-held-connections` (backlog row "Held egress connections pending a `network-access` decision" in `product/iterations/roadmap.md`).
- Webhook events for tasks without an active run still show up only when the message is delivered in the next run. Rows without a run are already in the backlog ("Further factory activity in the transcript … factory activity outside runs").
- Rewriting old webhook bubbles in stored transcripts: rejected (pre-MVP).
