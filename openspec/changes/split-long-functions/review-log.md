# Review log: split-long-functions

- BASE: 9e660c262c53d915db3d98bf5ffc9df2c57a7555
- Store: none
- Status: done
- Implementer agent: aed14411e7d76f651

## Round 1 (2026-10-29)

- Implementer: ee7f6a5 (14 commits 5cd6d7e..ee7f6a5), tasks 21/21, checks fmt/clippy (cargo + bazel lint)/machete/xtask/build/tests all pass, container tests ran (Podman); `cargo test -p dark-factory` run via Bazel only (wasm32-wasip2 rust-std missing locally)
- Reviewed range: BASE...ee7f6a5

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-web/src/egress/log/page.rs:57`, `assets/egress-log.js:3` | `egress-log.js` (defer, in body) runs after `alpine.min.js` (defer, head); Alpine starts in a microtask after its script, so `alpine:init` fires before the listener is added and `egressLog` is undefined: traffic page interactivity broken (verified by reading load order + vendored Alpine `queueMicrotask(()=>{mr.start()})`) | fix-now | fixed (a7fe275) |
| P2 | Spec | Blocking | `crates/df-web/src/assets.rs:104` | D4 asks for a test that the asset is served; only the embed lookup is tested, no HTTP request to `/assets/egress-log.js` | fix-now | fixed (a7fe275) |
| S1 | Standards | P1 | `crates/factory-agent/src/runner.rs:361-449` | new private types placed before `pub async fn run_session` (public before private) | fix-now | fixed (b0c0a37) |
| S2 | Standards | P1 | `crates/dark-factory/src/main.rs:27-32` | `use sandbox::…` lines interleaved with `mod` lines | fix-now | fixed (1946007) |
| S3 | Standards | P1 | `crates/dark-factory/src/sandbox.rs:29-95` | private `DbClusterTargetsProvider`, `K8sServices` before `pub(crate)` items | fix-now | fixed (a372e65) |
| S4 | Standards | P1 | `crates/df-web/src/egress/log/page.rs:251`, `egress/tests/mod.rs` | duplicated test helper `script()` / `component_script()` | fix-now | fixed (a7fe275) |
| S5 | Standards | P1 | `crates/df-db/src/tasks.rs:166,182` | "negative task number" `u64::try_from` check done twice | fix-now | fixed (1ca32f4) |
| S7, P4 | Standards, Spec | P1 | `crates/factory-agent/src/runner.rs:33` | `pub use runtime_handler::…` re-export kept only for tests; move mixed into the logic commit | fix-now | fixed (b0c0a37) |
| S6 | Standards | P3 | `crates/factory-agent/src/runner.rs:563,659,711` | phase fns return `Result<_, String>` | none | cosmetic |
| S8 | Standards | P3 | `crates/df-k8s/src/provider.rs` | param `name` means `namespace/service` | none | cosmetic |
| S9 | Standards | P3 | `crates/df-agent-channel/src/tools.rs` | handler `set_title` not named after its tool | none | cosmetic |
| S10 | Standards | P3 | `crates/fake-acp-agent/src/agent.rs` | boolean flag `text_chunk_update(text, bool)` | none | cosmetic |
| S11 | Standards | P3 | `crates/factory-agent/src/runner.rs` | `TurnResult` built then patched with `wall_clock_exceeded` | none | cosmetic |
| P3 | Spec | P3 | commit 5cd6d7e | D3's three steps in one commit; WIP commits get squashed | none | cosmetic |
| P5 | Spec | P3 | - | D1 covering tests not listed per function; no uncovered branch found | none | cosmetic |

User decision: Fix blocking + P1 (P1, P2, S1–S5, S7/P4).

## Round 2 (2026-10-29)

- Implementer: 1ca32f4 (a7fe275, b0c0a37, 1946007, a372e65, 1ca32f4), tasks 21/21, checks fmt/clippy/machete/xtask/tests pass, container tests ran (df-db, df-web); browser check of /egress/log in obscura: `egressLog` defined, pause toggles Live→Paused
- Implementer side effect: removed local rustup wasm32-wasip2 target files of toolchain 1.99.1 (rustup metadata still lists it); repair with `rustup toolchain uninstall 1.99.1-aarch64-unknown-linux-gnu` + `rustup toolchain install`
- Reviewed range: ee7f6a5...1ca32f4

Ledger check: P1, P2, S2, S3, S4, S5, S7/P4 confirmed fixed. S1 fixed for the types the change added; the remaining private-before-public order in runner.rs is unchanged from BASE (see S12).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S12 | Standards | P1 | `crates/factory-agent/src/runner.rs:42-361` | pre-existing private helpers (`AbortOnDrop`, `send_body`, `IdleSignals`, …) still before `pub` items; order identical at BASE | fix-now | fixed (9cad8a6) |
| S14 | Standards | P1 | `crates/df-web/src/egress/log/page.rs:27-28` | orphan comment among constants, attached to no item | fix-now | fixed (c351136) |
| P6 | Spec | P1 | `openspec/changes/split-long-functions/design.md` D4, task 7.1 | D4 says the page includes the script; it is now loaded by the layout head before Alpine | fix-now | fixed (f38862a) |
| S13 | Standards | P3 | `crates/df-db/src/tasks.rs:113-115,174-178` | `insert_task` takes the number twice (`TaskKey` u64 + `allocated_number` i64 for the bind) | none | cosmetic |

User decision: Fix P1s (S12, S14 by implementer; P6 by orchestrator).

## Round 3 (2026-10-29)

- Implementer: c351136 (9cad8a6 S12 move-only, verified by identical sorted line multiset; c351136 S14), checks fmt/clippy/machete pass, tests factory-agent 116+1, df-web 378 (Podman)
- Orchestrator: f38862a (P6 wording in design.md D4 and task 7.1)
- Reviewed range: 1ca32f4...c351136

Ledger check: S12, S14, P6 confirmed fixed. No new findings on either axis.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S15 | Standards | P3 | `crates/factory-agent/src/runner.rs` `AbortOnDrop` doc | pre-existing "(Finding 10)" reference in a doc comment, moved unchanged | none | cosmetic |

## Finish (2026-10-29)

- Bazel lockfile build: not needed (no `Cargo.toml`/`Cargo.lock` changed).
- Tech debt: none (no P2 findings).
- Open blocking: none. Rounds: 3 reviews, 2 fix rounds.
- Container tests: all ran with the Podman socket; `cargo test -p dark-factory` ran through Bazel only (local wasm32-wasip2 target broken, see Round 2).
