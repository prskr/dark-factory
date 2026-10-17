# Design

## Context

Wire contract: `contract/proto/darkfactory/agent/v1/agent.proto` (delivery semantics in its header), `contract/README.md`. Run lifecycle: `design/agent-run.md` §4, §6. Shared schema, notifications, traits and fakes: `iter1-foundation` design D3–D5. The real `SandboxProvider`/`SecretStore` (`iter1-sandbox-provisioning`) and `ForgeClient` (`iter1-integration-plugins`) are developed in parallel; this change uses the fakes until they land.

## Goals / Non-Goals

**Goals:**
- Stateless per connection: any replica can serve any reconnect; all decisions come from the database.
- Full behaviour testable in-process: a test drives a real tonic server with a scripted runtime client (mTLS certs from `rcgen`) plus the fakes.

**Non-Goals:**
- `AwaitingInput`, blocking questions, approvals, cancel button (protocol `Cancel` is used only for budget enforcement).
- Leader election — nothing in this change needs a single writer.
- Mirroring progress to issues; per-project budgets (instance defaults only).

## Decisions

### D1 — Module layout of `df-agent-channel`
`server` (tonic service, TLS, identity), `session` (one connected stream: seq/ack, command rebuild, prompt push), `scheduler` (start runs, timeouts, provider events), `transcript` (persistence), `tools` (registry + four handlers), `pr` (lifecycle), `assign` (building `Assign`/`HarnessCredentials` from DB rows and config).

```mermaid
sequenceDiagram
  participant S as scheduler
  participant P as SandboxProvider
  participant R as runtime
  participant C as session
  S->>P: ensure_task_volume, provision (run pending→provisioning)
  P-->>S: Started{pod_ip} (→connecting, runs.pod_ip)
  R->>C: Hello (mTLS; resolve_identity → run)
  C->>R: HelloAck, Assign, HarnessCredentials, Prompts
  R->>C: PhaseChanged / transcript events / PushReport (seq)
  C->>R: ack (after persist), ToolCallResult, CredentialGrant
  R->>C: RunReport
  C->>P: teardown; task status from outcome
```

### D2 — Scheduling
A loop per replica woken by `task_message_queued`/`run_changed` notifications and a 15 s tick: `SELECT … FROM tasks WHERE status='queued' AND NOT EXISTS (active run) FOR UPDATE SKIP LOCKED LIMIT n`, insert run (`pending`), set task `running`, commit, then call the provider. The partial unique index from foundation is the final guard. Provider events update runs idempotently on every replica. Timeouts are checked in the same tick from timestamps in `runs`.

### D3 — Stream session
Inbound: messages are persisted in batches (≤ 50 messages or 50 ms) in one transaction with `ON CONFLICT DO NOTHING`, `runs.last_persisted_seq` advanced, then `ack` sent. A seq gap closes the stream with `DATA_LOSS`. Outbound: on connect and on `task_message_queued` for the bound task, undelivered prompts are (re-)sent; the runtime ignores duplicates. Tool calls and credential requests are handled in spawned tasks so a slow forge doesn't stall acks.

### D4 — Outcome → run state
`completed`, `end_turn`, `limit_reached`, `rate_limited` → `succeeded`; `cancelled` → `cancelled`; `failed`, `provisioning_failed`, `connect_timeout`, `sandbox_lost` → `failed`. Task status per `specs/run-orchestration/spec.md`.

### D5 — Summary fallback
`Continuation.summary` = the task title and description, then the last 20 user/agent text messages of the transcript, truncated to 16 KiB. *ponytail:* no model-generated summary; pluggable strategies come with F3.

### D6 — Tool execution
Arguments validated with `jsonschema` against the embedded factory-tools file. `tool_calls` row inserted first (`ON CONFLICT (call_id) DO NOTHING`, then `SELECT … FOR UPDATE`); a row with a result returns it, otherwise the handler runs and stores the result. Handlers get `(TaskRow, args, &dyn ForgeClient)`.

### D7 — PR lifecycle
Triggered from the session after persisting a `PushReport` and after `RunReport`. Uses `find_open` before `create` and again on `IntegrationError::Conflict`. PR comments use `add_issue_comment` with the PR number (Forgejo PRs are issues).

### D8 — Harness defaults
Instance configuration `harness.claudeCode`: default adapter command, non-secret env (e.g. `DISABLE_AUTOUPDATER=1`), default `settings_json`; persona fields override. Budgets: `runs.maxWallClock` (default 2 h), `runs.connectTimeout` (default 10 min), `runs.cancelGrace` (default 5 min).

## Risks / Trade-offs

- [Several replicas hold streams for the same run after a network split] → dedup by (run, seq) makes double delivery harmless; prompts are idempotent.
- [Forge latency inside tool calls] → handlers run off the stream loop (D3); the harness waits, the stream doesn't.
- [Summary fallback is crude] → only used when `session/load` fails; acceptable for iteration 1.
