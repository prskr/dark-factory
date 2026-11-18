# Design

## Context

See proposal.md for motivation. State of `df-agent-channel` as `db-write-ownership` and `sandbox-contract-and-tls` leave it (module dependencies measured on today's code):

| Module | Uses (crate-internal) | Context and config used | Goes to |
|---|---|---|---|
| `scheduler` | `runs`, `assign::load_run_context` | `pool`, `notifier`, `provider`, `config.{runs, scheduling, events}` | `df-orchestrator` |
| `runs` (rest: `finish_run`, `teardown`) | none | `pool`, `provider`, `config` (end-run values) | `df-orchestrator` |
| `retention` | `runs` state lists | `pool`, `provider`, `config.{volumes, events}` | `df-orchestrator` |
| `pr`, `pr/sync`, `pr/mark_draft` | none | `pool`, `forge` | `df-orchestrator` |
| `webhooks` | none | own `pool` + `forge` (`WebhookApplier`) | `df-orchestrator` |
| `assign` | `runs::RunRow`, `summary` | `pool`, `config.{git_proxy, harness, landlock}` | split: run context to `df-orchestrator`, `Assign` message to the port |
| `summary` | `assign::RunContext`, `metrics::label` | `pool` | port |
| `session` | `assign`, `runs`, `scheduler::cancel_deadline`, `pr`, `tools`, `transcript`, `metrics` | `pool`, `notifier`, `config.runs` | port |
| `tools` (+ `artifacts`, `delegation`, `feedback`) | `assign`, `pr::repo_ref` | `pool`, `forge`, `approval_types` | port |
| `server`, `metrics` | `runs`, `session` | `pool` | port |

No orchestration module uses a port module. `dark-factory` uses `Scheduler`, `Retention`, `webhooks::WebhookApplier`, `pr::sync`, `pr::mark_draft`, `Metrics`, `serve` and the test fixtures (`testing::Env`).

## Goals / Non-Goals

**Goals:**
- `df-orchestrator` does not depend on `df-agent-channel`; `df-agent-channel` depends on `df-orchestrator`.
- Code moves without changes in behavior. Signatures change only where a function took `ChannelContext`/`ChannelConfig` and now takes the orchestration part.

**Non-Goals:**
- See proposal, Out of Scope.

## Decisions

### 1. Untangle inside the crate first, then move

Step one runs entirely in `df-agent-channel`. It splits `assign.rs`, `ChannelContext`, `ChannelConfig` and `ChannelError` into orchestration and port parts, so that orchestration modules only see their part. The compiler then proves the boundary while everything is still one crate, and the existing tests run unchanged. Step two moves the modules.

Alternative: move first and fix the errors. Rejected: the move and the untangling would be one large diff that is hard to review.

As built: the untangle and the move landed in one pass, the compiler enforcing the boundary (the orchestration modules name no port type, checked by grep). The types are flat in `df_orchestrator` (`lib.rs` for context and config, `error`, `run_context`, `testing`), without an `orchestration` module.

### 2. Context and config composition

```rust
// df-orchestrator
pub struct OrchestratorContext { pub pool, pub notifier, pub provider, pub forge, pub secrets, pub config: Arc<OrchestratorConfig> }
pub struct OrchestratorConfig { pub runs, pub scheduling, pub volumes, pub events }

// df-agent-channel
pub struct ChannelContext { pub orch: OrchestratorContext, pub config: Arc<ChannelConfig>, pub metrics, pub approval_types }
pub struct ChannelConfig { pub agent, pub harness, pub git_proxy, pub artifacts, pub landlock }
```

The port reaches shared parts through `ctx.orch`. Plain structs, no traits (decision 1 of `db-write-ownership` applies).

As built: `ChannelContext` implements `Deref<Target = OrchestratorContext>`, so the roughly 170 uses of `ctx.pool`, `ctx.provider`, `ctx.forge`, ... in the port stay unchanged. `ctx.config` is the `ChannelConfig` (a field of `ChannelContext` wins over the deref), so the orchestration config is always written `ctx.orch.config`. The channel's test `Env` derefs to `df_orchestrator::testing::Fixture` (seeded rows and fakes) and owns the single `ChannelContext`; `with_events` and `other_replica` are shared free functions of `df_orchestrator::testing`.

Alternative: one context type in `df-orchestrator` that the port also uses. Rejected: orchestration would carry port-only fields (metrics, approval types, harness settings).

### 3. `assign.rs` split

`RunContext`, `ProjectInfo`, `PersonaInfo` and `load_run_context` go to `df_orchestrator::run_context`. The scheduler needs them to provision a sandbox. `build_assign`, `tool_descriptors`, `tool_input_schemas`, `pending_prompts`, `inference_base_url`, `proxy_repo_url` and the private helpers stay in `df_agent_channel::assign`. They build the wire message.

### 4. Errors

`OrchestratorError` takes the variants orchestration raises (`Db`, `Sqlx`, `Provider`, `Volume`, `Secret`, `Integration`, `InvalidData`, `Json`, `Parse`). `ChannelError` gets `Orchestrator(#[from] OrchestratorError)` and keeps only its port-only variant (`Request`). Every failure kind has exactly one variant: `?` on `DbError`, `sqlx::Error`, `serde_json::Error` and `ParseEnumError` in port code goes through explicit `From` impls on `ChannelError` that wrap the `OrchestratorError` variant (a blanket impl would conflict with core's `From<T> for T`), and port code raises invalid data as `OrchestratorError::InvalidData`. The gRPC status mapping of `ChannelError` maps the wrapped variant like the variant it replaces, so clients see the same codes.

### 5. Test fixtures

`df_orchestrator::testing` (`test-util`) owns database seeding and the fakes wiring (`Env` without the server). `df_agent_channel::testing` builds on it and adds the PKI (from `df-tls`), the server on an ephemeral port and the scripted runtime client. `dark-factory` tests switch to whichever they need.

## Risks / Trade-offs

- [A hidden orchestration → port call found during the move (for example through `ChannelContext` methods)] → The crate split enforces the boundary: a hidden orchestration → port call shows up as a compile error in `df-orchestrator` and is resolved by moving the function or passing a value.
- [`ChannelError` → gRPC status mapping changes by accident] → A test pins the status code for one wrapped variant per kind (database, provider, invalid data).
- [Large diff in `dark-factory` wiring] → Wiring only; covered by `cargo nextest run -p dark-factory` and `just e2e-local`.

## Migration Plan

Code-only refactor, no configuration or Helm change. Rollback is a revert.
