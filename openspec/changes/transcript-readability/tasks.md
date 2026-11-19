# Tasks

## 1. Design agreement (PenPot)

- [ ] 1.1 Draw in PenPot, in light, dark and phone width: the thinking row (collapsed and open), the tool call group (collapsed, open with nested rows, running, with failures), the webhook rows (review, comment, CI failure, PR closed) and the tool details (shell line, JSON input, Markdown output, diff). Verify: the user approves the board, and any changed label or icon is written back into `specs/task-ui/spec.md` before group 4 starts (design D1).

## 2. Pending question scroll bug

- [ ] 2.1 Write a failing e2e test in `crates/df-e2e` (skill `e2e`): open a task whose transcript ends with a pending question, at desktop and 390 px width, and assert that the document does not scroll, the card is fully in view and nothing blank follows it. Repeat with a network-access card appended live. Verify: the test fails on `main` for the reported case (design D6).
- [ ] 2.2 Find the cause, fix it in the scroll container or layout CSS (`df-web/src/task_pages.rs`, `styles/input.css`), and record the cause and fix in `design.md` D6. Verify: the 2.1 tests pass, and the existing "Transcript follows the latest item" tests stay green.

## 3. GitHub Markdown extensions

- [ ] 3.1 Write tests first in `markdown.rs` for every scenario of "GitHub Markdown extensions" (table, bare URL incl. `javascript:`, task list with raw HTML next to it), plus one artifact view test with a table in Preview. Then enable `table`, `strikethrough`, `tasklist` and `autolink` in `options()` (design D7). Verify: `cargo nextest run -p df-web markdown artifacts`, and the existing "Untrusted Markdown" tests stay green.

## 4. Thinking rows

- [ ] 4.1 Rewrite `c::thought` in the tool row layout (`Icon::Brain`, "Thinking", first-line preview, chevron, `data-tool`), and render the body as Markdown once its turn has ended. Write the tests first in `components/transcript.rs` and `transcript/mod.rs`: preview text, chevron, plain text while streaming, a highlighted fence after the turn ends. Verify: `cargo nextest run -p df-web`.

## 5. Readable tool input and output

- [ ] 5.1 Replace the `(label, String)` blocks of `render_tool` with typed blocks: JSON input pretty-printed and highlighted, a leading `command` shell line, Markdown output once the call is completed or failed, highlighted diff, JSON result, and plain text for invalid JSON (design D5). Write tests first for every scenario of "Readable tool input and output", including the script-in-output case. Verify: `cargo nextest run -p df-web`.

## 6. Tool call groups

- [ ] 6.1 Add group membership to `transcript::Folder` (design D3: tool calls open or join a group, thoughts and notices join an open one, everything else closes it). Write tests first: a fold of all events and an event-by-event fold produce the same groups, a single call has a group of one, and a message splits a streak. Verify: `cargo nextest run -p df-web transcript`.
- [ ] 6.2 Render groups: a wrapper with head (count, failed count, running indicator, `aria-expanded` button), an items container, the head hidden for one call, and Alpine open state on the wrapper. Place request cards that fall inside a streak after the group. Write tests first for the "Tool call groups" and "Expanding a tool call group" scenarios. Verify: `cargo nextest run -p df-web`.
- [ ] 6.3 Stream groups over SSE (`df-web/src/sse.rs`): new members go `beforeend` into the items container, the head is morphed on count or status changes, and the wrapper is never morphed. Write tests first for 1 → 2 → N tool calls, a failed call updating the head, and a card raised during a streak. Verify: `cargo nextest run -p df-web sse`.
- [ ] 6.4 Add an e2e test: a seeded streak, expanding the group and a nested row, a live tool call arriving while both are open, and both staying open. Verify: `just e2e-local` (skill `e2e`).

## 7. Webhook events as forge activity rows

- [ ] 7.1 Add `ForgeActivity.webhook_received` (`WebhookReceived` with its oneof per kind, no payload text) to `transcript.proto`, plus the mapping from `df_core::webhooks::WebhookEvent`, with unit tests per kind. Verify: `cargo build --workspace` and `cargo nextest run -p df-core webhooks`.
- [ ] 7.2 Add a `df-db` migration for `task_messages.forge_event BYTEA NULL`. `queue_event_tx` stores the text as a hidden block and the encoded event in the column. Regenerate `.sqlx/`. Write tests first (`df-db` webhooks tests): the block is hidden, the column holds the event, and other system messages have none. Verify: `cargo nextest run -p df-db webhooks` (container-backed; state it if no socket is available).
- [ ] 7.3 In the delivery-confirmation path that persists a `UserMessage`, append the `webhook_received` control event with source key `webhook:<prompt_id>` when the message has a `forge_event`. Write tests first: one event per delivery, none on redelivery, none for `task.assigned`, and none for a runtime `UserMessage` claiming `system(webhook)` without a stored event. Verify: `cargo nextest run -p df-db transcript` (container-backed).
- [ ] 7.4 Fold and render `ForgeItem::Webhook` with `Icon::Webhook`, using one summary function shared with the queued stack. Links are only rendered for `http`/`https` URLs. Write tests first for the webhook row scenarios, the unsafe-scheme scenario and the queued-stack row. Verify: `cargo nextest run -p df-web`.

## 8. Documentation

- [ ] 8.1 Update `docs/content/using/tasks.md` (transcript: thinking rows, tool call groups, tool details, webhook rows, queued webhook rows), `docs/content/using/artifacts.md` (Markdown tables, strikethrough, task lists and autolinks) and `docs/content/integrations/forges/webhooks.md` (the wake-up text is hidden from the transcript and shown as a row on delivery). Verify: `just docs-build`.

## 9. Integration checks

- [ ] 9.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete` and `bazel --bazelrc=.bazelrc.agents test //crates/df-web/... //crates/df-db/...`, then check the chat UI transcript by hand (`just e2e-local` seed) for groups, thinking and webhook rows and a Markdown table. Verify: all commands pass, and the chat transcript shows the same rows as the task transcript.
