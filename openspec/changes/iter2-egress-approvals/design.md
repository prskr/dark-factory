# Design

## Context

- The allowlist (`egress_allowlist`), its notification (`egress_policy`) and the snapshot come from `iter1-egress-gateway`. The snapshot sends each sandbox entry its effective `HostRule`s, already resolved per run.
- Approval types plug into `iter2-agent-requests` through `ApprovalType`.
- Denials happen in the gateway, which has no database.

## Goals / Non-Goals

**Goals:**
- One function creates `http-host` entries (form, log action, approval). It also settles covered requests, so every path behaves the same.

**Non-Goals:**
- `cluster-service`, CIDR and persona scopes through approvals or the log.
- Persisting denial events (the log stays ephemeral).

## Decisions

### D1 — Task scope
`egress_allowlist.task_id` (nullable). A check constraint allows exactly one of instance / project / task scope, and task scope only for `http-host`. The snapshot query adds `OR e.task_id = r.task_id`. `HostRule.scope` gains the value `task`. No proxy change: the proxy matches the resolved rule list.

### D2 — Denial reports
```proto
service GatewayEvents { rpc ReportDenial(Denial) returns (google.protobuf.Empty); } // control plane, 9444
message Denial { string run_id = 1; string host = 2; uint32 port = 3; google.protobuf.Timestamp at = 4; }
```
The gateway keeps a `HashMap<(run, host, port), Instant>` and reports at most once per minute per key. Reports are fire-and-forget with one retry; a lost report only delays the request until the next denial. The map is pruned when runs leave the snapshot. The control plane raises the request (raised by the control plane, non-blocking, reason "The egress proxy denied <host>:<port>").

### D3 — Entry creation and settling
`create_http_host_entry(tx, scope, host, port, actor)` inserts the entry, writes the audit entry and publishes `egress_policy`. Then it selects pending `network-access` requests with fingerprint `host:port` whose task falls under the scope, and decides them through the decision path of `iter2-agent-requests` with `apply` skipped (the entry already exists), as decider = the actor. The `network-access` type's `apply` calls the same function, which therefore has to skip re-settling the request being decided.

Wildcard entries (`*.suffix`) also settle requests for matching hosts. Port must match exactly.

### D4 — Tool
`request_approval` in `factory-tools.v2.json` declares `type` as an enum with `network-access` and `payload` as an object validated per type by the control plane, since JSON Schema `oneOf` per type is poorly supported by harnesses. The handler first checks the run's effective allowlist and returns "already allowed" without storing anything.

### D5 — Log action
Denied events in the SSE log render a small menu (task / project / instance) when the event has a run. Summary rows aggregated over several tasks offer project (if they share one project) and instance. The action posts to the allowlist handler with `host` and `port` taken from the event, never a wildcard.

## Risks / Trade-offs

- [Harness telemetry hosts flood the inbox with requests] → one request per task and host thanks to merging. The board member can deny, or add an instance entry, which settles all of them. Harness settings should disable telemetry (`design/egress.md` §11).
- [Approving at instance scope from a phone is too easy] → the scope picker preselects task (narrowest); instance is the last option.
- [Lost denial reports] → acceptable: the next denial reports again, and the log still shows the event.
