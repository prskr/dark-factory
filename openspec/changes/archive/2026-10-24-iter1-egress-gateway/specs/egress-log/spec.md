# Spec Delta

## Purpose

A live, ephemeral view of every connection attempt through the egress proxy, built for spotting hosts a harness needs but isn't allowed to reach (`design/egress.md` §8).

## ADDED Requirements

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
The desktop-only egress log page SHALL show events live with filters (denied only, project, task, persona, host) and a denied-hosts summary (host, port, count, last seen, affected projects).

#### Scenario: Missing domain
- **WHEN** a Claude run is repeatedly denied `statsig.anthropic.com`
- **THEN** the denied-hosts summary lists it with its count and project
