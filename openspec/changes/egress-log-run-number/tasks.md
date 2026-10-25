# Tasks

In every task, write the failing test first, then the code. Database-backed tests need a container socket; list the ones that could not run in your report.

## 1. Run number in the registry and the events

- [ ] 1.1 Add `uint32 run_number` to `SandboxEntry` and `FlowEvent` in `gateway.proto` (new field numbers, with a comment that 0 means unknown). Verify that `cargo build --workspace` and the Bazel proto targets build.
- [ ] 1.2 Select `r.number` in `df_db::active_runs` (`ActiveRun.run_number`) and regenerate `.sqlx/`. Verify with a database test that an active run comes back with its number.
- [ ] 1.3 Copy the number into `SandboxEntry` (`control/snapshot.rs`) and into the flow event (`Flow::start`). Verify the `egress-policy-distribution` scenario "Gateway connects" and the `egress-log` scenario "Run number recorded" in the gateway's snapshot and proxy tests.

## 2. Traffic page

- [ ] 2.1 Change `sandbox_label` (design D2: `DF-7 #2`, short run id when the number is 0, source IP for an unknown source) and the graph node lines (`#2` / `DF-7`). Verify the `egress-log` scenarios "Node content", "Unknown source", "Event without a run number" and "Row content" in rendered tests.
- [ ] 2.2 Give the `--demo-flows` events run numbers, and update `docs/design-system.md` (graph node and events table). Verify that `cargo test -p dark-factory --features dev-tools` passes and that `just serve-local` shows `#N` in graph and table.

## 3. Integration checks

- [ ] 3.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --workspace` (with a container socket) and `openspec validate egress-log-run-number --strict`. Verify that all three pass.

## Workflow follow-up

- Archive `network-management-ui-polish` before this change; this change modifies its egress-log requirements.
