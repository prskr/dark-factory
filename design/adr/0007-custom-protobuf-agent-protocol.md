# ADR-0007: Custom protobuf protocol between runtime and control plane

- **Status:** Accepted (amended by ADR-0027 and ADR-0029)
- **Date:** 2026-10-16
- **Related:** [ADR-0017](0017-transcript-schema-and-versioning.md), [ADR-0027](0027-agent-protocol-delivery-and-typed-tool-calls.md)

## Context

ACP is the protocol between the runtime and the harness. Using it toward the control plane as well was considered, but the control plane needs registration, assignment, credential leasing, resume, artifacts and tool tunnelling.

## Decision

- A custom protobuf protocol: one stream `Connect(stream AgentMessage) returns (stream ControlMessage)` with `oneof` payloads and sequence numbers/acks.
- Agent → control plane: hello, phase changes, transcript events, tool calls, credential requests, push and run reports (later: artifacts).
- Control plane → agent: assign, harness credentials, prompts, cancel, tool results, credential grants.
- Both harness modes normalize into Dark Factory's own transcript schema (ADR-0017).
- The runtime serves the factory tools to the harness and forwards their calls; the control plane authorizes them per task and executes them (database, run control, WASI plugins). Third-party MCP servers are a data path through the gateway (ADR-0032).
- **Scope:** the protocol is the control channel between runtime and control plane only. Data paths don't use it: model inference (full-harness mode), HTTP egress and, later, Git go through the gateway Deployment using their native protocols (OpenAI-compatible HTTP, HTTP `CONNECT`, Git smart HTTP), see ADR-0029. The gateway's own link to the control plane is a separate internal API.

## Consequences

- ACP stays contained in the runtime; the control plane speaks one protocol regardless of harness.
- Inference stays out of the protocol, so any harness that accepts a base URL can use the inference endpoint, not only the built-in loop; streaming tokens don't pass through the control plane.
- Contract: `contract/proto/darkfactory/agent/v1/agent.proto`.

## Alternatives considered

- **ACP end to end**: roles assume an editor that owns filesystem and terminal; no concepts for assignment, credentials, resume or artifacts.
- **Tunnelling inference through the agent stream** (the original plan): would put token streams through the control plane and only work for the built-in loop; superseded by the gateway's inference endpoint (ADR-0029).
