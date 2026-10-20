# agent-channel Specification

## Purpose

The control plane end of the agent protocol v1 (`contract/proto/darkfactory/agent/v1/agent.proto`): who may connect, how a connection is bound to a run, and the delivery guarantees in both directions (ADR-0006, ADR-0027).

## Requirements

### Requirement: Mutual TLS on a dedicated port
The agent port SHALL accept only TLS connections presenting a client certificate issued by the instance's private CA. It SHALL be separate from the UI port.

#### Scenario: No client certificate
- **WHEN** a client connects without a certificate
- **THEN** the TLS handshake fails

#### Scenario: Certificate from another CA
- **WHEN** a client presents a certificate from a different CA
- **THEN** the TLS handshake fails

### Requirement: Identity bound to the sandbox pod
The control plane SHALL extract the URI SAN `spiffe://dark-factory/ns/<ns>/pod/<name>/<uid>` and the peer IP, and SHALL accept the stream only if they resolve to an active run: the pod exists, its UID matches, it belongs to a Sandbox created by this instance, and the peer IP equals the pod IP.

#### Scenario: Stolen key used elsewhere
- **WHEN** a valid certificate is presented from an IP that isn't the pod's IP
- **THEN** the stream is rejected with `UNAUTHENTICATED`

#### Scenario: Pod of a finished run
- **WHEN** a pod whose run is terminal connects
- **THEN** the stream is rejected with `UNAUTHENTICATED`

### Requirement: Handshake
The first agent message SHALL be `Hello`; the control plane SHALL answer `HelloAck` with the bound run id, the last persisted sequence number and its capabilities, followed by `Assign`, `HarnessCredentials`, every undelivered prompt of the task and `Cancel` if one was requested. A different `previous_run_id` in `Hello` SHALL NOT change the binding.

#### Scenario: First connect
- **WHEN** the runtime of run R connects for the first time
- **THEN** it receives `HelloAck { run_id: R, last_persisted_seq: 0 }`, then `Assign`, `HarnessCredentials` and the assignment prompt

#### Scenario: Message before Hello
- **WHEN** the first message on a stream isn't `Hello`
- **THEN** the stream is closed with `FAILED_PRECONDITION`

### Requirement: Reliable agent-to-control-plane delivery
The control plane SHALL acknowledge cumulatively only after persisting, SHALL drop messages whose sequence number is already persisted, and SHALL close the stream on a sequence gap.

#### Scenario: Replay after reconnect
- **WHEN** the runtime reconnects and replays seq 5–9 while 5–7 were already persisted
- **THEN** 5–7 are dropped, 8–9 are persisted once and the ack is 9

#### Scenario: Gap
- **WHEN** a stream delivers seq 12 after seq 10
- **THEN** the stream is closed with `DATA_LOSS` and nothing after 10 is persisted

### Requirement: Idempotent commands after reconnect
Commands to the runtime SHALL be derived from the database, so a reconnect to any replica produces the same `Assign` and the same pending prompts, tool results and credential grants.

#### Scenario: Reconnect to another replica
- **WHEN** replica A restarts and the runtime reconnects to replica B
- **THEN** B sends the same `Assign` and re-sends prompts not yet confirmed as delivered

### Requirement: Message size limit
The agent port SHALL accept messages up to 16 MiB and reject larger ones with `RESOURCE_EXHAUSTED`.

#### Scenario: Oversized message
- **WHEN** a 20 MiB message arrives
- **THEN** the stream fails with `RESOURCE_EXHAUSTED`
