# Spec Delta

## MODIFIED Requirements

### Requirement: Flow events in a ring buffer
Each gateway pod SHALL record one closing flow event per proxy request in an in-memory ring buffer (default 10,000 events) with: time, gateway pod, flow id, source IP, run (id and number), task, project, persona, method, host, port, SNI, phase, verdict, reason, matched rule, bytes in/out, duration and HTTP status for plain HTTP. Nothing SHALL be persisted per event.

#### Scenario: Buffer full
- **WHEN** event 10,001 is recorded
- **THEN** the oldest event is dropped

#### Scenario: Run number recorded
- **WHEN** run 2 of task `DF-7` opens a connection
- **THEN** its flow events carry the run's id and the number 2

### Requirement: Graph node cards
Graph nodes SHALL be cards with an icon tile on the panel: a sandbox node shows the run number (`#2`) and the task key, a destination node its host icon, host and port. A node of an unknown source SHALL show its source IP. The sandbox column, heading included, SHALL sit in a dashed group box, and the shorter column SHALL be centred vertically on the taller one.

#### Scenario: Node content
- **WHEN** the graph shows run 2 of task `DF-7` and destination `github.com:443`
- **THEN** the sandbox node is a card with an icon tile, `#2` and `DF-7`, and the destination node shows the `GH` icon, `github.com` and `443`

#### Scenario: Unknown source
- **WHEN** a connection comes from `10.0.0.9`, which is no active sandbox
- **THEN** its sandbox node shows `10.0.0.9`

#### Scenario: Event without a run number
- **WHEN** an event of a known run carries no run number (an older gateway)
- **THEN** the sandbox node shows the first 8 characters of the run id

### Requirement: Events table
The events table SHALL be headed "Events · newest first" with "N of M" (events matching the filters of events seen) and list per event: time (UTC), sandbox (task key and run number, `DF-7 #2`), destination (host icon, `host:port`), a verdict pill (*Allowed*, *Blocked*, *Open*) and actions. Bytes and duration SHALL be shown in the request details, not in the table.

#### Scenario: Row content
- **WHEN** run 2 of task `DF-7` is denied `203.0.113.42:22`
- **THEN** the row shows "DF-7 #2", `203.0.113.42:22` and a *Blocked* pill, and the details panel names the sandbox "DF-7 #2"

#### Scenario: Missed events
- **WHEN** the stream reports a `gap` because the page could not keep up
- **THEN** the table shows a "Events are missing" row regardless of the filters, and the gap adds no edge and does not change "N of M" or the segment counts

#### Scenario: Row actions
- **WHEN** an allowed event has a matched rule
- **THEN** its row offers *Request details* and a link to the rule; blocked rows offer *Request details* only
