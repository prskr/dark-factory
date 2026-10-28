# Proposal

## Why

`split-oversized-test-modules` introduced the 100-line function limit, but did not split the 17 production functions that exceed it. Each carries an `#[expect(clippy::too_many_lines, reason = "<planned split>")]`. The splits were listed only as follow-ups in that change's design, which is now archived, so nothing tracks them.

The worst cases are `run_session` (743 lines), `build_agent` (374), `render_sandbox` (289) and `run_loop` (277). They hold the core control flow of the sandbox runtime and the control plane. Every feature in these areas makes them longer and harder to review.

Iteration 1 is done. This change pays down that debt before iteration 2 adds more features on top.

## What Changes

- **Splits:** each of the 17 functions is split into steps of at most 100 lines, and its `#[expect(clippy::too_many_lines)]` is removed. The split named in the attribute's `reason` is the starting point.
- **`df-web` traffic page:** the Alpine component in `egress/log/page.rs` moves from a Rust format string into a static JS asset.
- **`dark-factory/src/main.rs`** (1,459 lines): the sandbox backend wiring (`SandboxBackend`, `KubernetesBackend`, `DbClusterTargetsProvider`, `K8sServices`, `resolve_cluster_ips`) moves into its own module while `KubernetesBackend::start` is split.
- **Behavior:** unchanged. Existing tests are the safety net. Where a path a split touches has no test, a characterization test is written first.
- **End state:** no production function carries `#[expect(clippy::too_many_lines)]`. Module-level opt-outs in test modules stay.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. Pure refactor (`skip_specs: true`).

## Impact

- Production code of `factory-agent`, `fake-acp-agent`, `df-k8s`, `df-agent-channel`, `df-db`, `df-web` and `dark-factory`
- `df-web/assets/` (new JS asset for the traffic page)
- **Order:** implement after `lint-and-crate-guardrails` and before `clippy-strictness`, so the strictness fixes land on the split functions and are not done twice.
