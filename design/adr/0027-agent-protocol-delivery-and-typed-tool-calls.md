# ADR-0027: Agent protocol delivery semantics and typed tool calls

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0006](0006-agent-dial-back-grpc-mtls.md), [ADR-0007](0007-custom-protobuf-agent-protocol.md), [ADR-0032](0032-upstream-mcp-through-gateway-with-tool-catalog.md)

## Context

Drafting `agent.proto` raised two questions ADR-0007 left open: how delivery works across reconnects when the control plane has several replicas, and whether tool calls cross the stream as raw MCP frames.

## Decision

- **Agent → control plane is reliable and ordered.** Every message has a per-run `seq`; the runtime keeps it in an on-disk outbox until acknowledged; the control plane acknowledges only after persisting and drops duplicates. `HelloAck.last_persisted_seq` tells a reconnecting runtime where to resume.
- **Control plane → agent is idempotent, not sequenced.** Commands are keyed (`run_id`, `prompt_id`, `call_id`, `request_id`) and rebuilt from the database after every reconnect, so any replica can serve it.
- **Request/response pairs** are re-sent by the runtime after a reconnect with the same id; the control plane answers from its stored result.
- **Prompt delivery** is confirmed by the transcript `UserMessage` event carrying the `prompt_id`.
- **Typed tool calls for factory tools** (`ToolCallRequest`/`ToolCallResult`) instead of raw MCP frames. Factory tools are those that act on Dark Factory's own state (PR/issue updates through plugins, progress, completion, later delegation, approvals, artifacts); third-party MCP servers don't use the agent stream, they go through the gateway (ADR-0032). The runtime answers `tools/list` from `Assign.tools`; `call_id` (UUIDv7) is the idempotency key and audit record id. `ToolCallResult.end_run_after_turn` lets the control plane end a run (used by `complete_task`).

## Consequences

- No per-connection state in the control plane; reconnects work across replicas.
- Factory tools don't need MCP features beyond tools (resources, prompts, elicitation); upstream servers keep them because they're reached with native MCP through the gateway.
- Contract and details: `contract/README.md`.

## Alternatives considered

- **Sequence numbers in both directions**: the control plane's outbox would have to be persisted or would be lost when a reconnect lands on another replica.
- **Raw MCP frames**: needs per-run MCP session state in the control plane and makes idempotent retries harder.
