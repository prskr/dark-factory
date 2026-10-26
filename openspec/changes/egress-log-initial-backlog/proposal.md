# Proposal

## Why

`/egress/log` is rendered empty: an empty graph (`page.rs:286`) and an empty table body (`page.rs:322`). Rows, graph and counts arrive only with the first SSE message. The page therefore flashes "No network traffic yet" on every load, and without a working stream (a headless browser that does not read the fetch-streamed SSE, a broken proxy) it shows nothing at all. UI states cannot be compared or checked from the page HTML.

## What Changes

- The page handler reads the current backlog (the same stream the SSE handler reads, up to `backlog_end`) and renders rows, graph, counts and the empty state into the HTML response.
- The SSE stream is unchanged: its first message still replaces rows, graph and counts with the backlog. The page shows the same content twice in a row; what changed in between is picked up.
- Pause, resume and filter changes keep working as before.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `egress-log`: the "Live log page" requirement (the page as served already shows the buffered events) and "Relay across gateway pods" (streams stay open for a grace period after the last viewer).

## Impact

- `crates/df-web/src/egress/log/mod.rs` (`page`), `page.rs` (`page_body`, `traffic`, `events_panel`), `viewer.rs` (backlog state usable outside an SSE message).
- The relay keeps its session for 5 s after the last viewer (`SESSION_GRACE`), so the event stream that follows a page load reuses the page's `Tail` streams instead of opening new ones; the streams to the pods stay open up to 5 s longer. The page response waits at most the relay's backlog wait (2 s, `crates/df-gateway/src/control/relay.rs`).
- `crates/df-gateway/src/control/relay.rs` (session grace period), `design/egress.md` (stream lifetime).
- `docs/design-system.md`: the `/egress/log` paragraph.
- Independent of `egress-log-run-number`; both modify different `egress-log` requirements.
