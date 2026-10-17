# Proposal

## Why

The control plane has to turn pending work on a task into a run, talk to the runtime over the agent protocol, persist what the agent does, execute the factory tools and open the draft PR. This is the coordinator at the centre of the demo (milestones 4–6 of `product/iterations/iteration-1.md`, control-plane side).

## What Changes

- Agent port: tonic gRPC server with rustls (ring), client certificates from the private CA, identity check through `SandboxProvider::resolve_identity` (pod UID, Sandbox ownership, peer IP).
- Protocol v1 server: `Hello`/`HelloAck` with capability negotiation, per-run sequence/ack with dedup, idempotent command rebuild after reconnect, prompt delivery.
- Run scheduler and state machine: start a run when a task is `queued` and has no active run, provision through `SandboxProvider`, track phases, enforce connect and wall-clock timeouts, tear down, map outcomes to task status, continuation (`session/load` id or summary).
- Transcript persistence with blob extraction and prompt delivery confirmation.
- Factory tools v1 execution (`update_pull_request`, `comment_on_issue`, `report_progress`, `complete_task`) with idempotency by `call_id` and audit entries.
- Pull request lifecycle: draft PR on the first pushed task branch ahead of base, stored title/body, ready for review on completion.
- Credential delivery: Claude subscription token as `HarnessCredentials`, Git credentials for the project repository only.

## Capabilities

### New Capabilities
- `agent-channel`: authenticated, resumable agent stream and its delivery semantics.
- `run-orchestration`: when runs start and end, their states, timeouts and the resulting task status.
- `transcript-storage`: how transcript events are persisted and confirm prompt delivery.
- `factory-tools`: the tools agents call to act on Dark Factory and their effects.
- `pull-request-lifecycle`: when and how the control plane opens and updates a task's pull request.
- `run-credentials`: which credentials a run receives and under which conditions.

### Modified Capabilities
None.

## Impact

- New crate `df-agent-channel`; `dark-factory serve` starts the agent port (9443) and the scheduler.
- Consumes `SandboxProvider`, `ForgeClient`, `SecretStore` from `iter1-foundation` — developed and tested with the fakes; real implementations come from `iter1-sandbox-provisioning` and `iter1-integration-plugins`.
- Embeds `contract/mcp/factory-tools.v1.json`.
- Writes `runs`, `transcript_events`, `tool_calls`, task status/PR columns read by `iter1-control-plane-web` and `iter1-egress-gateway` (pod IPs for the sandbox registry).
