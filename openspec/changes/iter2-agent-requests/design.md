# Design

## Context

- Factory tool calls are executed exactly once by the control plane and audited (`factory-tools`). Queued messages and their delivery already exist (`task-management`, `run-orchestration`). A decision only needs to queue one more message.
- The transcript is sequenced per run by the runtime (`transcript-storage`). The control plane does not write transcript events.
- The agent protocol already has `RUN_PHASE_AWAITING_INPUT` and `RUN_OUTCOME_BLOCKED`. Neither is used yet.

## Goals / Non-Goals

**Goals:**
- Approval types can be added by later changes without touching the decision flow, inbox or delivery.
- Deciding takes one tap for the common case.

**Non-Goals:**
- Concrete approval types (`push-check-exception` comes in `iter2-push-checks`; `network-access` and `request_approval` in `iter2-egress-approvals`).
- Questions to other agents, agents deciding approvals, push notifications.
- Closing requests on task cancellation (`iter2-cancel`).

## Decisions

### D1 — Storage
One table `agent_requests`:

| Column | Notes |
|---|---|
| `id` | UUIDv7 |
| `task_id`, `run_id` | `run_id` is null when the control plane raised the request |
| `kind` | `question` \| `approval` |
| `approval_type` | null for questions |
| `payload` | JSONB; question: `{text, suggested_answers}`; approval: type-defined |
| `reason` | agent-written reason for approvals |
| `blocking` | bool |
| `fingerprint` | text; questions: SHA-256 of the normalized text |
| `state` | `pending` \| `answered` \| `approved` \| `denied` \| `closed` |
| `decision` | JSONB `{answer}`, `{scope}` or `{comment}` |
| `decided_by`, `decided_at`, `close_reason` | |

A partial unique index on `(task_id, kind, coalesce(approval_type,''), fingerprint) WHERE state = 'pending'` implements merging (`INSERT … ON CONFLICT DO NOTHING RETURNING`, then select). The pending limit is checked with a count in the same transaction, under the task row lock that tool calls already take.

*Alternative:* request and decision events in the transcript (contract README "planned additive changes"). Rejected for now: the control plane would need its own sequence in a runtime-sequenced stream. The UI merges both sources by time instead (D5).

### D2 — Approval type registry
```rust
#[async_trait::async_trait] // boxes the future so the trait stays dyn-compatible
pub trait ApprovalType: Send + Sync {
    fn name(&self) -> &'static str;
    fn validate(&self, task: &Task, payload: &serde_json::Value) -> Result<(), RequestError>;
    fn fingerprint(&self, payload: &serde_json::Value) -> String;
    fn scopes(&self) -> &'static [Scope];               // narrowest first
    async fn apply(&self, tx: &mut PgTransaction, req: &Request, scope: Scope) -> Result<(), RequestError>;
    fn describe(&self, req: &Request) -> String;         // text for UI card and decision message
}
```
`async fn` in a trait is not dyn-compatible. `async-trait` (already a workspace dependency, used in `df-core` and `df-agent-channel`) turns `apply` into a boxed `Send` future, and every implementation uses the same attribute. Types are registered at start-up in a `BTreeMap<&str, Arc<dyn ApprovalType>>`. A raise with an unknown type is rejected. `Scope` is a closed enum (`Run`, `Task`, `Project`, `Instance`). A type offers a subset of it. Tests register a fake type.

### D3 — Decision transaction
Lock the request row (`FOR UPDATE`) and check `state = 'pending'` (otherwise "already decided"). Then, for an approval, `apply`. Then update the state and decision, write the audit entry, queue the `request.decided` message through the existing queued-message function (which sets the task to `queued` if no run is active), and publish `request_changed` and `task_message_queued` after commit. If `apply` fails, the whole transaction rolls back.

### D4 — Runtime awaiting input
`ToolCallResult.await_input_after_turn` is set by the control plane for blocking requests. After the turn, the runtime decides in this order: `end_run_after_turn` → finalize `COMPLETED`; queued prompts → next turn; `await_input_after_turn` → `AwaitingInput`. `AwaitingInput` is the idle wait of `iter1-keep-alive-and-review` (`select!` over prompt, cancel, `Release` and the budget); this change adds no runtime timer. The control plane sends `Release` at the deadline; the runtime finalizes `BLOCKED` instead of `END_TURN` when the last turn had a blocking request. The harness process stays up, so the next turn reuses the ACP session.

### D5 — UI
- `/requests` lists pending requests. Each card is a maud component shared with the task timeline. Actions are htmx `POST`s with the CSRF header. The response swaps the card in place.
- Live updates: the existing SSE infrastructure subscribes to `request_changed`. The inbox re-renders the changed card; the navigation count is a small fragment re-fetched on each event.
- Task timeline: request cards are merged into the transcript render by `created_at`. On reconnect replay, cards are re-sent as a whole (they are few per task).
- The scope picker is a radio group with the narrowest scope preselected, so approval is one tap plus "Approve".

### D6 — Configuration
No new setting: the deadline for blocking questions is `runs.keepAliveSeconds` (default 600), enforced by the control plane (`iter1-keep-alive-and-review`). There is no `Assign.keep_alive`.

## Risks / Trade-offs

- [Keep-alive holds a sandbox and the subscription session idle] → the window is short and configurable; after it, the run ends and the answer starts a new run.
- [Agents ask too many questions] → the 20-pending limit and merging; the agent playbook says when to ask.
- [Request cards and transcript can interleave oddly when clocks differ between replicas] → both timestamps come from the database (`now()`), not from the runtime.

## Migration Plan

The migration adds the table, the task status `blocked` and the run state `awaiting_input` to the check constraints. Rollback is not possible once rows with the new statuses exist; the previous release would reject them. Roll forward instead.
