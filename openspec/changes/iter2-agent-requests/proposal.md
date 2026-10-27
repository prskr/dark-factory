# Proposal

## Why

In iteration 1 an agent that needs a human can only write a question in plain text and end its turn. Nothing tracks the question, nothing tells the board member, and anything the agent needs (a network host, a push exception) has to be applied by hand. ADR-0038 defines one request model for questions and typed approvals. Iteration 2 needs it for the Git proxy's `push-check-exception` and for `network-access` (`product/iterations/iteration-2.md`).

## What Changes

- **Request model:** questions (free text, optional suggested answers) and approval requests (typed payload, reason). Each request is blocking or non-blocking, belongs to one task and is stored with its state (`pending`, `answered`, `approved`, `denied`, `closed`). Identical pending requests of a task are merged. Every request and decision is audited.
- **Approval types as a registry:** this change provides the decision flow and the framework. Each type defines payload validation, the fingerprint used for merging, the offered scopes and the action applied on approval. `iter2-push-checks` adds `push-check-exception`, and `iter2-egress-approvals` adds `network-access` together with the `request_approval` tool.
- **Factory tool `ask_question`** (factory tools v2). It is addressed to humans only in this iteration.
- **Decisions reach the agent as messages:** a system-authored queued message (reason `request.decided`). It is delivered in the next turn of a live run, or it starts a new run.
- **`AwaitingInput`:** reuses the idle wait and `Release` of `iter1-keep-alive-and-review`. After a turn with a blocking request the control plane keeps the run alive until its own deadline (default `runs.keepAliveSeconds`, 10 min) and sends `Release`; the run then ends `BLOCKED` (instead of `END_TURN`) and the task becomes `blocked`.
- **Inbox UI:** pending requests across tasks with a count in the navigation, plus request cards on the task timeline. Humans answer a question with text or one tap on a suggested answer, approve with a scope, or deny with a comment. Works on a phone (ADR-0035).
- **Hygiene:** pending requests are closed when the task completes.

## Capabilities

### New Capabilities
- `agent-requests`: raising, merging, deciding, delivering and closing questions and approval requests.
- `request-inbox`: how humans see and decide requests in the UI.

### Modified Capabilities
- `factory-tools`: new `ask_question` tool.
- `acp-harness`: the runtime waits in `AwaitingInput` after a blocking request, and `BLOCKED` becomes a run outcome.
- `run-orchestration`: `awaiting_input` run state, `blocked` outcome mapping, blocked-question deadline in the control plane (no `Assign` field).
- `task-management`: new task status `blocked`.

## Impact

- Contract: `agent.proto` gets `ToolCallResult.await_input_after_turn` (`Release` and `AWAITING_INPUT` come from `iter1-keep-alive-and-review`). New `contract/mcp/factory-tools.v2.json` = v1 + `ask_question`.
- Code: `df-db` migration (`agent_requests`, task status `blocked`, run state `awaiting_input`), the control plane request service and tool handler, the run outcome mapping, `df-web` inbox and cards, `factory-agent` awaiting-input loop.
- Instance configuration: none; reuses `runs.keepAliveSeconds`.
- **Depends on** `iter1-control-plane-web` (layout, auth, task detail) and `iter1-keep-alive-and-review` (idle wait, `Release`, `in_review`) being archived.
- `iter2-cancel` modifies the same status requirements and must be archived after this change.
