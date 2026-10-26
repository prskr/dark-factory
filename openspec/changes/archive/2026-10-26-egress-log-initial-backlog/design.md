# Design

## Context

See `proposal.md`. Current state:

- `log::page` renders `page::page_body`, whose `traffic` holds an empty graph (`graph::Model::default()`) and an empty `#flows tbody`; the counts and the empty state are those of `Counts::default()`.
- `log::events` subscribes to the `FlowSource`, collects events up to `backlog_end`, builds a `Viewer` and sends `Viewer::backlog_message` (rows, graph, counts as `<hx-partial>`s) as the first SSE message, then live messages.
- The relay ends the backlog after at most 2 s (`BACKLOG_WAIT`, `crates/df-gateway/src/control/relay.rs`), even if a gateway pod has not answered.

## Goals / Non-Goals

**Goals:**
- For the same backlog, the served page renders exactly what the first SSE message renders. Events that arrive between the page's and the stream's subscription are in the stream's newer backlog, and its first message replaces the page with it.

**Non-Goals:**
- Sending only live events over SSE after a server-rendered page (a cursor or resume token).
- Rendering the page without JavaScript beyond the initial state (filters and pause still need the stream).

## Decisions

### D1. The page reads its own backlog; the stream stays unchanged

`log::page` subscribes, collects up to `backlog_end` and drops the stream. The SSE handler is not changed: its first message replaces rows, graph and counts again.

*Alternative:* hand the stream's position to the SSE connection so it only sends live events (a per-pod cursor through the connect URL and `Last-Event-ID`). Rejected by [ADR-0042](../../../design/adr/0042-live-views-resume-only-from-an-authoritative-sequence.md): the gateway buffers have no authoritative sequence, so a cursor depends on per-pod ordering and clocks, fails silently, and the page would no longer correct itself on reconnect. Sending the backlog twice is a few hundred rows of HTML per page load.

### D2. One backlog state, two renderings

`Viewer` gets one method that applies the backlog and returns its state (rows markup, graph markup, counts). `backlog_message` wraps that state in partials; `page_body`/`traffic` render it in place of the defaults. The markup of rows, graph, counts and empty state is therefore produced by the same code on both paths, and a test can compare them.

*Alternative:* render the page from the SSE message by parsing it. Rejected: indirect and harder to test.

### D3. No extra timeout

The page waits for `backlog_end` like the SSE handler. The relay already bounds that wait (2 s); the demo source and test sources end their backlog immediately.

### D4. The relay keeps its session for a grace period

The page reads the backlog and drops its stream; the event stream subscribes right after. Without a grace period the relay would close its `Tail` streams at the first drop and start a new session (with a new wait for the pods' backlog, up to 2 s) for the event stream. `Relay` therefore keeps the session alive for `SESSION_GRACE` (5 s) after the last viewer left, through a linger task it owns (replaced when the next viewer leaves, aborted when the relay drops). A subscribe within the window upgrades the weak reference and reuses the warm session.

## Risks / Trade-offs

- [A slow gateway pod delays the page response by up to 2 s] → The same delay applies today before the first rows appear; it only moves from the stream to the response. Filter changes (`hx-get` of `/egress/log` with `hx-select="#traffic"`) reload the page and wait for the backlog by the same bound.
- [Events between the page's backlog and the stream's backlog] → The stream's first message replaces rows, graph and counts, so the page ends in the stream's state.
- [One more `Tail` per gateway pod per page load] → None when the event stream follows within the grace period (D4): both use the same session. The streams to the pods stay open up to 5 s longer after the last viewer.

## Migration Plan

None. No API, schema or protobuf change.
