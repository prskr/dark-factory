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
