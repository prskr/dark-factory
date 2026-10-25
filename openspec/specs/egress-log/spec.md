# egress-log Specification

## Purpose
A live, ephemeral view of every connection attempt through the egress proxy, built for spotting hosts a harness needs but isn't allowed to reach (`design/egress.md` §8).

## Requirements

### Requirement: Flow events in a ring buffer
Each gateway pod SHALL record one closing flow event per proxy request in an in-memory ring buffer (default 10,000 events) with: time, gateway pod, flow id, source IP, run, task, project, persona, method, host, port, SNI, phase, verdict, reason, matched rule, bytes in/out, duration and HTTP status for plain HTTP. Nothing SHALL be persisted per event.

#### Scenario: Buffer full
- **WHEN** event 10,001 is recorded
- **THEN** the oldest event is dropped

### Requirement: Opened events
An allowed request (a `CONNECT` tunnel, or plain HTTP once the response headers arrived) SHALL additionally record an event with phase `opened` as soon as the destination is connected; its closing event has phase `closed`. Denied requests SHALL record only the closing event.

#### Scenario: Long-lived tunnel visible while open
- **WHEN** a run opens a tunnel to `api.anthropic.com` and keeps it open
- **THEN** the live log shows it as open within a second, and the same row turns into a closed connection with bytes and duration when the tunnel ends

### Requirement: Reasons and verdicts
Every closing event that was not carried out normally SHALL carry a reason. Denied: `no-snapshot`, `unknown-source`, `ip-literal`, `not-allowlisted`, `private-destination`, `sni-mismatch`, `removed-from-allowlist`. Allowed (the policy let the request through, it failed or ended later): `resolve-failed`, `upstream-failed`, `upstream-timeout`, `tunnel-error`, `gateway-shutdown`. Only denied events SHALL count for the denied-only filter and the denied-hosts summary.

#### Scenario: Gateway rollout
- **WHEN** a gateway pod stops while runs have open tunnels to allowlisted hosts
- **THEN** their events have reason `gateway-shutdown` and verdict allowed, and the denied-hosts summary does not list those hosts

### Requirement: Tail stream
Each gateway pod SHALL serve `Tail(filter)` on its mTLS internal port, sending matching buffered events first, then a `backlog_end` marker, then live events. A subscriber that falls behind SHALL receive a `gap` marker instead of the events it missed.

#### Scenario: Tail with denied-only filter
- **WHEN** the control plane calls `Tail` with filter `verdict = denied`
- **THEN** only denied events are streamed

### Requirement: Relay across gateway pods
For each viewer, a control plane replica SHALL open `Tail` to every gateway pod, merge buffered events by time, interleave live events, and stream them to the browser via SSE; streams SHALL exist only while viewers are connected and SHALL follow gateway pods appearing and disappearing.

#### Scenario: Two gateway pods
- **WHEN** runs connect through two different gateway pods
- **THEN** the viewer sees events from both in one time-ordered stream

#### Scenario: Last viewer leaves
- **WHEN** the last viewer on a replica closes the page
- **THEN** that replica closes its `Tail` streams

### Requirement: Live log page
The desktop-only network traffic page (navigation label "Network traffic") SHALL show events live in a split view of a connection graph and an events table, with the filters of "Traffic filters". Denied connections SHALL be visible in the graph and the events table; there SHALL be no separate denied-hosts summary.

#### Scenario: Missing domain
- **WHEN** a Claude run is repeatedly denied `statsig.anthropic.com`
- **THEN** the graph shows it as a blocked destination and the *Blocked* segment lists each attempt

### Requirement: Page header
The page header SHALL show the title "Network traffic", a badge with the stream state (*Live*, *Paused*, *Reconnecting*), a *Pause*/*Resume* button and a link to the egress rules page.

#### Scenario: Live stream
- **WHEN** the event stream is connected
- **THEN** the badge reads *Live* and the button reads *Pause*

#### Scenario: Link to the rules
- **WHEN** a board member activates "Egress rules" in the header
- **THEN** the egress rules page opens

### Requirement: Pause and resume
*Pause* SHALL close the viewer's event stream, so the page stops changing and the gateway pods stop streaming to it. *Resume* SHALL reconnect, replacing rows, graph and counts with the buffered backlog and live events.

#### Scenario: Pause
- **WHEN** a board member pauses while runs keep connecting
- **THEN** no row, node or count changes and the badge reads *Paused*

#### Scenario: Resume
- **WHEN** the board member resumes
- **THEN** events that occurred during the pause appear and the badge reads *Live*

### Requirement: Traffic filters
The page SHALL offer, right-aligned, a domain or IP search (substring of the host), a verdict control *All / Blocked / Allowed* with the number of matching events per segment, and *Advanced* with a count badge of the advanced filters in use: a persona chosen by name and a task key (any case). *Clear filters* SHALL show while a filter is set and reset all. Filters SHALL be part of the page URL; `denied=1` SHALL select *Blocked*.

#### Scenario: Search narrows rows and graph
- **WHEN** a board member types `github` into the search
- **THEN** the table lists only events whose host contains `github`, and graph nodes and edges that do not match are dimmed

#### Scenario: Segment counts ignore the verdict
- **WHEN** the search is `pypi` and 3 matching events are blocked and 12 allowed
- **THEN** the segments read *All 15*, *Blocked 3*, *Allowed 12*, whichever segment is selected

#### Scenario: Open connections
- **WHEN** a tunnel is open
- **THEN** it counts as allowed and its verdict pill reads *Open*

#### Scenario: Shared link
- **WHEN** a board member opens a URL with `?verdict=blocked&q=pypi`
- **THEN** the page shows these filters applied

#### Scenario: Old link
- **WHEN** a board member opens `/egress/log?denied=1&project=DF`
- **THEN** *Blocked* is selected and the dropped `project` parameter is ignored

#### Scenario: Persona and task
- **WHEN** a board member opens *Advanced*, picks persona "Linus" and types `df-7` as task
- **THEN** the table lists only events of task `DF-7` run by Linus, and the badge reads 2

### Requirement: Split view with draggable divider
The graph and the events table SHALL sit side by side, separated by a divider that resizes them by pointer drag, by arrow keys (Home/End for the limits) and resets on double-click. The graph SHALL never be narrower than the width that shows its nodes untruncated, and the table SHALL keep at least 520 px. The width SHALL be remembered per browser.

#### Scenario: Drag
- **WHEN** a board member drags the divider
- **THEN** the panels resize within their limits and text selection is suppressed during the drag

#### Scenario: Keyboard
- **WHEN** the divider has focus and the board member presses Right Arrow
- **THEN** the graph grows by one step and the divider's `aria-valuenow` follows

#### Scenario: Reset
- **WHEN** the board member double-clicks the divider
- **THEN** the default width is restored

#### Scenario: Remembered width
- **WHEN** a board member resizes the divider and reloads the page
- **THEN** the same width is restored; if the browser cannot store it, the default width is used

### Requirement: Connection graph
The graph SHALL draw sandboxes (one per run, or per source IP for unknown sources) in a left column and external `host:port` destinations in a right column, with an edge per sandbox and destination. An edge SHALL be solid grey when its latest event was allowed and dashed red with a red destination border when it was denied. A legend and column headings with counts SHALL be shown. Sandboxes and destinations that the filters exclude SHALL be dimmed, not removed.

#### Scenario: Blocked destination
- **WHEN** sandbox `a4c2…` was denied `files.pythonhosted.org:443`
- **THEN** a dashed red edge leads from its node to a destination node with a red border

#### Scenario: Live update
- **WHEN** a run connects to a host the graph does not show yet
- **THEN** the destination node and edge appear without a page reload

#### Scenario: Many destinations
- **WHEN** more than 30 distinct destinations were seen
- **THEN** the 30 most recently seen are drawn and a note names how many are not shown

#### Scenario: Filtered out
- **WHEN** the verdict control selects *Blocked*
- **THEN** destinations and edges of allowed events are dimmed

#### Scenario: Resized graph
- **WHEN** a board member drags the divider to widen or narrow the graph
- **THEN** every edge still ends with its arrowhead at the left edge of its destination node

### Requirement: Graph node cards
Graph nodes SHALL be cards with an icon tile on the panel: a sandbox node shows the short run id and the task key, a destination node its host icon, host and port. The sandbox column, heading included, SHALL sit in a dashed group box, and the shorter column SHALL be centred vertically on the taller one.

#### Scenario: Node content
- **WHEN** the graph shows sandbox `a4c2…` of task `DF-7` and destination `github.com:443`
- **THEN** the sandbox node is a card with an icon tile, `a4c2…` and `DF-7`, and the destination node shows the `GH` icon, `github.com` and `443`

### Requirement: Events table
The events table SHALL be headed "Events · newest first" with "N of M" (events matching the filters of events seen) and list per event: time (UTC), sandbox (short run id and task key), destination (host icon, `host:port`), a verdict pill (*Allowed*, *Blocked*, *Open*) and actions. Bytes and duration SHALL be shown in the request details, not in the table.

#### Scenario: Row content
- **WHEN** task `DF-7` is denied `203.0.113.42:22`
- **THEN** the row shows the run's short id, "DF-7", `203.0.113.42:22` and a *Blocked* pill

#### Scenario: Missed events
- **WHEN** the stream reports a `gap` because the page could not keep up
- **THEN** the table shows a "Events are missing" row regardless of the filters, and the gap adds no edge and does not change "N of M" or the segment counts

#### Scenario: Row actions
- **WHEN** an allowed event has a matched rule
- **THEN** its row offers *Request details* and a link to the rule; blocked rows offer *Request details* only

### Requirement: Row limit
The events table SHALL keep at most a chosen number of rows, the newest: 100, 250, 500 (default), 1000, 2500 or 5000, chosen in a *Rows* select of the filter bar and kept in the page URL. Older rows SHALL be dropped as new ones arrive. The counts and the graph SHALL still cover every event, and *Clear filters* SHALL keep the choice.

#### Scenario: Full buffer
- **WHEN** the page opens while a gateway pod holds 10,000 events
- **THEN** the table shows the newest 500 rows and "N of M" still counts all 10,000 events

#### Scenario: Live rows past the limit
- **WHEN** the table shows 500 rows and a new event arrives
- **THEN** the new row is on top and the oldest row is dropped

#### Scenario: Closing a dropped row
- **WHEN** the closing event of a tunnel arrives whose open row was already dropped
- **THEN** it is shown as a new row on top

#### Scenario: Other choice
- **WHEN** a board member chooses 1000 under *Rows*
- **THEN** the URL carries `rows=1000` and the table keeps up to 1000 rows

### Requirement: Narrow events table
When the events panel is narrower than 680 px, the verdict column SHALL be replaced by a coloured row edge (solid green for allowed and open, dashed red with a tinted row for blocked; the verdict stays available to assistive technology) and the actions SHALL collapse into a menu button per row.

#### Scenario: Narrow panel
- **WHEN** a board member drags the divider so that the events panel is 600 px wide
- **THEN** rows show no verdict column, blocked rows have a dashed red edge and each row has an actions menu

#### Scenario: Wide panel
- **WHEN** the events panel is at least 680 px wide
- **THEN** the verdict column and the inline actions are shown

### Requirement: Request details panel
A side panel opened from a row SHALL show: destination with host icon, time, sandbox and task; method, status (HTTP status for plain HTTP, otherwise the verdict), duration and bytes sent and received; SNI, source IP and gateway pod; the reason (code and explanation) of denied or failed events; and the matched rule with a link to it when one matched, otherwise "No rule matched".

#### Scenario: Open and close
- **WHEN** a board member opens the details of a row and then presses Escape or the close button
- **THEN** the panel closes and focus returns to the row's details button

#### Scenario: Allowed tunnel
- **WHEN** the details of a closed `CONNECT` tunnel to `api.anthropic.com:443` are opened
- **THEN** the panel shows method `CONNECT`, the bytes, the duration and the matched rule `api.anthropic.com`

#### Scenario: Denied request
- **WHEN** the details of an event denied with `not-allowlisted` are opened
- **THEN** the panel shows the reason code, its explanation and "No rule matched"

#### Scenario: Link to the matched rule
- **WHEN** the board member follows the matched rule link of an event matched by `*.crates.io` on port 443
- **THEN** the egress rules page opens with the rules for `*.crates.io:443` highlighted

#### Scenario: Open connection closes
- **WHEN** the panel shows an open tunnel and its closing event arrives
- **THEN** the panel shows the closing values (bytes, duration, reason) without being reopened

### Requirement: No request content in the details
The details panel SHALL state that requests inside TLS tunnels are not inspected, and SHALL NOT show URL path, query parameters, headers or response content type, which the gateway does not record.

#### Scenario: Tunnel details
- **WHEN** the details of any `CONNECT` event are opened
- **THEN** the panel shows the note "Requests inside TLS tunnels are not inspected." and no URL, query, header or response section

### Requirement: Empty states
With no event seen yet, the events panel SHALL say that nothing has tried to reach the network yet. With events seen but none matching the filters, it SHALL say so and offer *Clear filters*.

#### Scenario: No traffic yet
- **WHEN** the gateway buffers hold no events
- **THEN** the table shows "No network traffic yet" and the graph shows no nodes

#### Scenario: Nothing matches
- **WHEN** the filters exclude every event
- **THEN** the table shows "No events match these filters" with a *Clear filters* button, and the count reads "0 of M"

### Requirement: Project host icons
A destination whose host is a project's repository or issue tracker host SHALL show that project's icon in the events table, graph nodes and details panel. The repository host shows the project's forge icon, the same as in the project list. An issue tracker host shows the forge icon when it equals the repository host, otherwise the issue-tracker icon. Matching ignores case, a trailing dot and the port, and does not cover subdomains.

#### Scenario: Self-hosted forge
- **WHEN** the instance runs the forgejo integration, project `DF` with a forge credential has repository `https://git.example.internal/org/df.git`, and a sandbox connects to `git.example.internal:443`
- **THEN** the event row, the graph node and the details panel show the Forgejo icon for `git.example.internal`

#### Scenario: Project without forge credential
- **WHEN** project `WEB` without a forge credential has repository `https://code.example.org/web.git` and a sandbox connects to `code.example.org:443`
- **THEN** the destination shows the generic Git icon

#### Scenario: Issue tracker on its own host
- **WHEN** project `DF` has issue tracker `https://jira.example.com/browse/DF` and a sandbox connects to `jira.example.com:443`
- **THEN** the destination shows the issue-tracker icon

#### Scenario: Issue tracker on the repository host
- **WHEN** project `DF` has repository and issue tracker on `git.example.internal`
- **THEN** a connection to `git.example.internal:443` shows the forge icon, not the issue-tracker icon

#### Scenario: Subdomain of a project host
- **WHEN** project `DF` has repository host `git.example.internal` and a sandbox connects to `api.git.example.internal:443`
- **THEN** the destination does not show the project's icon

#### Scenario: Project host before provider monogram
- **WHEN** the instance runs the github integration, project `DF` with a forge credential has repository `https://github.com/org/df.git`, and a sandbox connects to `github.com:443`
- **THEN** the destination shows the GitHub icon instead of the `GH` monogram, and `api.github.com` keeps the `GH` monogram

#### Scenario: Project created while the page is open
- **WHEN** a project is created while the network traffic page is open
- **THEN** its hosts show the project icon after the page is reloaded
