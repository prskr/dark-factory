# runtime-connection Specification

## Purpose

The runtime end of the agent protocol v1: dialling the control plane with the pod's identity and delivering every message reliably across reconnects and control plane restarts (ADR-0006, ADR-0027).

## Requirements

### Requirement: Dial back with the pod certificate
The runtime SHALL connect to the control plane address from its environment, verify the server certificate against the instance CA and the configured server name, and present the pod certificate. It SHALL reload certificate files when they change and use the new ones for new connections.

#### Scenario: Certificate rotated
- **WHEN** csi-driver replaces the certificate files and the stream later drops
- **THEN** the reconnect uses the new certificate

### Requirement: Hello and resume
On every connect the runtime SHALL send `Hello` with its version, capabilities and the run id stored in `/data/runtime`, then replay every unacknowledged message after `HelloAck.last_persisted_seq` in order.

#### Scenario: Control plane restart
- **WHEN** the stream drops after the runtime sent seq 1–20 and the control plane persisted 1–15
- **THEN** after reconnecting the runtime sends 16–20 and continues with 21

#### Scenario: Leftover from a crashed run
- **WHEN** `HelloAck.run_id` differs from the stored run id
- **THEN** the old outbox is discarded and a `Notice` with code `runtime.outbox_discarded` is emitted

### Requirement: Durable outbox
Every message except `Hello` SHALL get a per-run sequence number starting at 1 and SHALL be written to the on-disk outbox before sending; acknowledged messages SHALL be removed. The outbox SHALL be bounded; when full, the runtime SHALL pause the harness until acknowledgements free space.

#### Scenario: Runtime restarted in the same pod
- **WHEN** the runtime process restarts mid-run
- **THEN** unacknowledged messages from the outbox are replayed after reconnect

### Requirement: Reconnect with backoff
On stream failure the runtime SHALL reconnect with exponential backoff (capped at 30 s) and continue the run while the harness keeps working.

#### Scenario: Short outage
- **WHEN** the control plane is unreachable for 60 seconds
- **THEN** the harness keeps working, events queue in the outbox and are delivered after reconnect

### Requirement: Idempotent command handling
The runtime SHALL ignore duplicate `Assign`, `Prompt` (same prompt id), `ToolCallResult` and `CredentialGrant` messages, and SHALL re-send pending tool-call and credential requests with the same ids after reconnecting.

#### Scenario: Prompt re-sent after reconnect
- **WHEN** the control plane re-sends a prompt the runtime already holds
- **THEN** it is delivered to the harness once
