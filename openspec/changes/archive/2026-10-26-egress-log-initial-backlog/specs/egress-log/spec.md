# Spec Delta

## MODIFIED Requirements

### Requirement: Live log page
The desktop-only network traffic page (navigation label "Network traffic") SHALL show events live in a split view of a connection graph and an events table, with the filters of "Traffic filters". Denied connections SHALL be visible in the graph and the events table; there SHALL be no separate denied-hosts summary. The page as served SHALL already show the buffered events (rows, graph, counts and empty state), before the event stream connects.

#### Scenario: Missing domain
- **WHEN** a Claude run is repeatedly denied `statsig.anthropic.com`
- **THEN** the graph shows it as a blocked destination and the *Blocked* segment lists each attempt

#### Scenario: Page without a stream
- **WHEN** the gateway buffers hold events and the browser loads the page but never receives a stream message
- **THEN** the page shows the rows, graph nodes and counts of the buffered events that match its filters

#### Scenario: Stream connects after load
- **WHEN** the event stream connects after the page has rendered the buffered events
- **THEN** the table shows each flow once, and graph and counts match the stream's backlog

### Requirement: Relay across gateway pods
For each viewer, a control plane replica SHALL open `Tail` to every gateway pod, merge buffered events by time, interleave live events, and stream them to the browser via SSE; streams SHALL exist only while viewers are connected, plus a short grace period (5 s) after the last viewer left, and SHALL follow gateway pods appearing and disappearing.

#### Scenario: Two gateway pods
- **WHEN** runs connect through two different gateway pods
- **THEN** the viewer sees events from both in one time-ordered stream

#### Scenario: Last viewer leaves
- **WHEN** the last viewer on a replica closes the page
- **THEN** that replica closes its `Tail` streams after the grace period (5 s)

#### Scenario: Stream follows page load
- **WHEN** the event stream subscribes within the grace period after the page read the backlog
- **THEN** it reuses the open `Tail` streams and does not wait for the backlog again
