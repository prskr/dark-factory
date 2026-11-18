# Tasks

Requires `db-write-ownership` and `sandbox-contract-and-tls`. Before starting, re-measure the module table in design.md on the current code; if a new orchestration → port use appeared, resolve it in group 1. Code moves with its tests; a new crate follows "Adding a crate" in the `rust` skill. No user- or operator-visible behavior changes, but moved modules change tracing targets: the published page `docs/content/integrations/forges/webhooks.md` names `df_orchestrator::webhooks` in its log filter. Contributor docs are task 4.1. Database tests need PostgreSQL; report the ones that could not run.

## 1. Untangle inside `df-agent-channel`

- [x] 1.1 Write a failing test pinning the gRPC status codes of `ChannelError` for a database, a provider and an invalid-data error. Add `OrchestratorError` (`df_orchestrator::error`, flat in the crate) with the orchestration variants and `ChannelError::Orchestrator`, keeping the codes. Verify `cargo nextest run -p df-agent-channel error`.
- [x] 1.2 Introduce `OrchestratorContext`/`OrchestratorConfig` (in `df_orchestrator`'s `lib.rs`) and make `ChannelContext`/`ChannelConfig` contain them (design decision 2). Switch `scheduler`, `runs`, `retention`, `pr`, `webhooks` to take only the orchestration part. Verify `cargo nextest run -p df-agent-channel` and that these modules no longer name `ChannelContext`, `ChannelConfig` or `ChannelError`.
- [x] 1.3 Split `assign.rs`: `RunContext`, `ProjectInfo`, `PersonaInfo`, `load_run_context` into `df_orchestrator::run_context`; the `Assign` builder stays. Verify `cargo nextest run -p df-agent-channel assign scheduler session`.
- [x] 1.4 Split `testing.rs` into orchestration fixtures (seeding, fakes, `Env` without server) and port fixtures on top. Verify `cargo nextest run -p df-agent-channel` and that no orchestration module or orchestration test imports `session`, `server`, `tools`, `summary`, `metrics` or the `Assign` builder (grep).

## 2. New crate

- [x] 2.1 Create `crates/df-orchestrator` (`Cargo.toml` with workspace lints, `BUILD.bazel` with library and unit-test targets, `no-remote-exec` on database tests, `test-util` feature, `MODULE.bazel` manifest entry, `[workspace.dependencies]` entry, lock files regenerated); verify `cargo xtask check-crates`.
- [x] 2.2 Move the context, config, error, run context and fixtures (flat modules `lib`, `error`, `run_context`, `testing`), `scheduler`, `runs`, `retention`, `pr`, `webhooks` with their tests into `df-orchestrator`; `df-agent-channel` depends on it. Verify `cargo nextest run -p df-orchestrator -p df-agent-channel` runs the same number of tests as before the move.

## 3. Wiring

- [x] 3.1 Switch `dark-factory` (`main.rs`, `forge.rs`, `outbox.rs`, tests) to build `OrchestratorContext` once and pass it to `Scheduler`, `Retention`, `WebhookApplier`, `pr::sync`, `pr::mark_draft` and into `ChannelContext`. Verify `cargo nextest run -p dark-factory` and that `cargo tree -p df-orchestrator | grep df-agent-channel` finds nothing.

## 4. Documentation and integration

- [x] 4.1 In `docs/architecture.md`, change the rows of `df-agent-channel` and `df-orchestrator` from **target** to current and check the graph against `cargo metadata --no-deps`. Update the project overview in `AGENTS.md` (`df-agent-channel` description, new `df-orchestrator` entry). Grep `docs/content`, `crates/*/docs` and `deploy/` for `df_agent_channel::` tracing targets of moved modules (the webhooks log filter in `docs/content/integrations/forges/webhooks.md`). Verify `cargo xtask check-crates`.
- [x] 4.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length`, `bazel --bazelrc=.bazelrc.agents test //crates/df-orchestrator/... //crates/df-agent-channel/... //crates/dark-factory/...` and `just e2e-local` (or report that it could not run); all pass.
