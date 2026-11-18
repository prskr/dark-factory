# Proposal

## Why

`df-agent-channel` is named after the agent protocol, but half of it is control-plane orchestration that has nothing to do with a connected agent: the run scheduler, sandbox provisioning and teardown, the PR lifecycle, forge webhook application and task volume retention. `dark-factory` reaches into it for `Scheduler`, `Retention`, `WebhookApplier` and `pr::{sync, mark_draft}`. Everything shares one `ChannelContext` and one `ChannelConfig`, so an orchestration change rebuilds and retests the gRPC server, and the reverse.

The modules already point one way: the agent port (`server`, `session`, `tools`) uses orchestration (`runs`, `scheduler::cancel_deadline`, `pr`, run context loading), and orchestration never uses the port. A crate boundary makes that direction binding. Target: `docs/architecture.md`, rows `df-agent-channel` and `df-orchestrator`.

## What Changes

- New crate `df-orchestrator`, containing:
  - `scheduler` (`Scheduler`, `cancel_deadline`)
  - the run lifecycle left in `runs` after `db-write-ownership` (`finish_run`, `teardown`)
  - run context loading (`RunContext`, `ProjectInfo`, `PersonaInfo`, `load_run_context`, taken out of `assign.rs`)
  - `pr` (with `sync` and `mark_draft`)
  - `webhooks` (`WebhookApplier`)
  - `retention` (`Retention`)
- `OrchestratorContext` holds `pool`, `notifier`, `provider`, `forge`, `secrets`, and `OrchestratorConfig` holds the run, scheduling, volume and lifecycle-event settings. `ChannelContext` and `ChannelConfig` hold these plus the port's own parts (harness, git proxy, Landlock, artifacts, approval types, metrics).
- `OrchestratorError` covers the orchestration errors; `ChannelError` wraps it.
- `df-agent-channel` keeps the agent port: `server`, `session`, `tools`, the `Assign` message (`build_assign`, tool descriptors, `pending_prompts`), the continuation `summary` and the run `metrics`. It depends on `df-orchestrator`, never the reverse.
- Test fixtures split the same way: database seeding and fakes in `df_orchestrator::testing` (`test-util`), the server and the runtime client in `df_agent_channel::testing` on top of it.
- `dark-factory` builds the orchestration parts from `df-orchestrator`.
- `docs/architecture.md`: the two rows change from **target** to current.
- No change in behavior, configuration or wire format.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. Pure refactor: `skip_specs: true` in `.openspec.yaml`.

## Out of Scope

- Renaming `df-agent-channel`. Rejected: after the split the name describes what is left.
- Moving the forge-calling tool handlers (`update_pull_request`, `comment_on_issue`) into `df-orchestrator`. Rejected: they are agent-facing tool handlers; they call `df-orchestrator` and `df-db` functions.
- Changing the run state machine (`df_db::runs`): change `run-state-machine` (ADR-0061). Best landed first, since it shrinks `runs` and `scheduler`.

## Impact

- New crate: `crates/df-orchestrator` (`Cargo.toml`, `BUILD.bazel` with `no-remote-exec` database tests, `MODULE.bazel` manifest entry, `[workspace.dependencies]`, lock files).
- Changed: `df-agent-channel` (`lib.rs`, `assign.rs`, `runs.rs`, `scheduler.rs`, `pr/`, `webhooks.rs`, `retention.rs`, `error.rs`, `testing.rs`, `session.rs`, `tools.rs`), `dark-factory` (`main.rs`, `forge.rs`, `outbox.rs`, tests).
- Contributor docs: `docs/architecture.md`, `AGENTS.md` (project overview). One published page changes: the log filter target `df_agent_channel::webhooks` becomes `df_orchestrator::webhooks` in `docs/content/integrations/forges/webhooks.md`.
- Dependencies: no new external crates.
- Order: after `db-write-ownership` (it moves the run, task and transcript SQL out of these modules, so far less code moves here) and after `sandbox-contract-and-tls` (TLS leaves the crate first). Independent of `gateway-control-crate`.
