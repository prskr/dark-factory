## MODIFIED Requirements

### Requirement: Flow events in a ring buffer
Each gateway pod SHALL record one closing flow event per proxy request in an in-memory ring buffer (default 10,000 events, default maximum age 1 hour) with: time, gateway pod, flow id, source IP, run (id and number), task, project, persona, method, host, port, SNI, phase, verdict, reason, matched rule, bytes in/out, duration and HTTP status for plain HTTP. Events older than the maximum age SHALL be dropped and SHALL NOT be sent as backlog, whatever their position in the buffer. The `opened` event of a connection that has not closed yet SHALL be kept regardless of age, subject to the count limit. Nothing SHALL be persisted per event.

#### Scenario: Buffer full
- **WHEN** event 10,001 is recorded
- **THEN** the oldest event is dropped

#### Scenario: Event too old
- **WHEN** a viewer subscribes and the buffer holds an event recorded 61 minutes ago
- **THEN** that event is dropped and not part of the backlog

#### Scenario: Long-lived tunnel
- **WHEN** a `CONNECT` tunnel opened 90 minutes ago is still open and a viewer subscribes
- **THEN** its `opened` event is part of the backlog and the connection shows as open

#### Scenario: Run number recorded
- **WHEN** run 2 of task `DF-7` opens a connection
- **THEN** its flow events carry the run's id and the number 2

## ADDED Requirements

### Requirement: Merged buffer has the same maximum age
The control plane's merged buffer of the gateway pods' events SHALL drop events older than the configured maximum age, with the same exception for `opened` events of connections that are still open, so a viewer that opens the page while a relay session is already running gets the same time window as from the gateway pods.

#### Scenario: Long-lived relay session
- **WHEN** a relay session has been open for two hours and a second viewer opens the page
- **THEN** its backlog contains only events of the last hour

### Requirement: Fixed header and scrolling panels
From the `md` breakpoint up, the page header and the filter bar SHALL stay in place while the board member scrolls; narrower screens keep the normal page flow. The split view SHALL fill the remaining viewport height; the connections panel and the events panel SHALL each scroll on their own, and the events table's column headers SHALL stay visible while its rows scroll. A graph taller than its panel SHALL scroll inside the panel and SHALL also move with a pointer drag on its background.

#### Scenario: Many events
- **WHEN** the table holds 500 rows and the board member scrolls down the events panel
- **THEN** the header, the filter bar, the graph and the table's column headers stay visible

#### Scenario: Tall graph
- **WHEN** the graph shows 12 sandboxes at 1000 px viewport height
- **THEN** the connections panel scrolls to reach the lower nodes and dragging its background moves the graph
