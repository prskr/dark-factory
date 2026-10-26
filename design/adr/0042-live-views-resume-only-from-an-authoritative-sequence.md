# ADR-0042: Live views resume only from an authoritative sequence

- **Status:** Accepted
- **Date:** 2026-10-26
- **Related:** [ADR-0004](0004-server-rendered-ui-maud-htmx-alpine.md), [ADR-0029](0029-egress-gateway-and-generated-network-policies.md)

## Context

The UI streams live views over SSE (ADR-0004). After a page load, a reconnect (the stream ends at every token refresh) or a resume, the server has to know what the page already shows. There are two ways:

- **Resume:** the page sends a cursor (`Last-Event-ID` or a query parameter) and the server sends only what came after it. The transcript does this: its events have one sequence number from PostgreSQL.
- **Snapshot:** every connection starts with the full current state, which replaces what the page shows, and continues with deltas.

The network traffic page (`/egress/log`) merges the in-memory ring buffers of several gateway pods (ADR-0029). There is no shared sequence. A cursor would be a time per gateway pod, and it would only be correct if:

- each pod's buffer stays in time order and its clock never steps back;
- a restarted pod, which has an empty buffer and a new name, is handled;
- every later change to the event model, the relay, the viewer state, filters and pause/resume keeps the cursor semantics.

A violation would not raise an error. It would show as a missing or duplicate row, reproducible only with several pods, clock skew and a badly timed reconnect, and it would stay on the page until a reload.

## Decision

- A live view resumes from a cursor only if its source has one authoritative, gap-free sequence (for example a database sequence, as for the transcript).
- Every other live view starts each stream with a full snapshot that replaces the page's state, then sends deltas. This applies to the network traffic page.
- If a page should not render empty before its stream connects, the server also renders the same snapshot into the page, through the same code. The stream's first message then replaces it once more.

## Consequences

- Every connection rebuilds its state from the source. A wrong state lasts at most until the next connection.
- The state needs no cursor format, no cursor validation and no per-source ordering assumptions.
- The snapshot is transferred once per connection, and twice on a page load that renders it server-side. For the traffic page that is at most the gateway buffers (10,000 events per pod, filtered to the row limit).
- Revisit when a snapshot gets expensive: much larger buffers, many concurrent viewers, or frequent reconnects. A resumable view then needs an exact sequence per writer first (see the `(pod incarnation, seq)` cursor below), not a cursor over timestamps.
- The SSE client is not a factor: the vendored `hx-sse` stores message ids as the SSE specification describes and sends `Last-Event-ID` on reconnect, so a resumable view needs no other client.

## Alternatives considered

- **Per-pod time cursor for the traffic page** (initial cursor in the connect URL, updated through SSE `id:` and `Last-Event-ID`, which the vendored `hx-sse` sends on reconnect): no duplicate transfer and cheaper reconnects. Rejected because correctness depends on the assumptions above, failures are silent and hard to reproduce, and the state no longer corrects itself.
- **Assign a global sequence in the control plane relay:** the sequence would be per relay connection and lost with it, so it cannot serve as a cursor across reconnects or control plane replicas.
- **Snowflake ids** (time, instance number, counter; for example with the control plane as a StatefulSet for stable instance numbers): unique and roughly time-ordered, but ordered by each generator's clock, so an event with a lower id can become visible after a higher one. That is the time cursor's problem again. The events also originate in the gateway pods, not in the control plane, which only sees them while a viewer is connected.
- **Per-pod exact cursor `(pod incarnation, seq)`:** each gateway pod numbers its events with a counter and an id that is new on every start (for example the pod UID); the cursor maps pod to last seq. Clocks no longer matter, a restarted pod is detected by its new incarnation, and the gateway knows exactly which numbers fell out of its buffer. Not chosen now because the cursor protocol (encoding, validation, pods coming and going, filters, pause/resume) is a lasting cost for a small gain and the page no longer corrects itself on reconnect. This is the design to use if the snapshot gets expensive.
- **Cursor kept server-side** (per page instance instead of in `Last-Event-ID`): the server knows what it sent, not what the browser received, so messages lost in a dropped connection would be skipped silently. It also needs storage shared by all control plane replicas, a key per browser tab and expiry for closed tabs. Pods coming and going need the same rules either way.
