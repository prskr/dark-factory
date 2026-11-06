# Review log: sandbox-lifecycle-resilience

- BASE: ab992ff3f346aaf12e8061f1722569a3d1c5c16a
- Store: none
- Status: done (container-backed tests ran; no blocking finding open; tech debt TD-121)
- Implementer agent: per-run (VS Code implementer subagents are stateless)
- Container socket: `DOCKER_HOST=unix:///Users/martinhusgen/.orbstack/run/docker.sock`
- Design tasks: none

## Implementation (2026-11-06)

| Iteration | Commit | Tasks | Checks |
|-----------|--------|-------|--------|
| 1 | "iteration 1" | 2.1, 2.2 | df-core 211, df-k8s 89, dark-factory 54, clippy, Bazel 4/4 |
| 2 | "iteration 2" | 2.3, 2.4 (behavior already held; tests pin it) | df-k8s 91, Bazel 1/1 |
| 3 | "iteration 3" | 2.5 | df-agent-channel 304+27+1 (container-backed ran) |
| 4 | "iteration 4" | 3.1, 3.2 (README image-pull part follows with 1.3) | df-agent-channel 308+27+1 (container-backed ran) |
| 5 | "iteration 5" | 1.1, 1.2 | df-k8s provider tests, fixtures recorded on kind 1.36.1/1.37.0 |
| 6 | "iteration 6" | 1.3 | df-k8s (tracker dedupe both orders) |
| 7 | "iteration 7" | 1.4, 1.5 | df-k8s startup, helm-unittest rbac |
| 8 | "iteration 8" | 4.1, 4.2, 1.6 record | live tests on kind (`--test-threads=1`) |
| 9 | aed731d1 | 4.3 | manual SNI check on kind |
| 10 | f8fdf6c7 | 5.1, 5.2 | docs build, links test |
| 11 | 00eeaecb | 6.1 | clippy, machete, Bazel 46 pass / 5 skipped (macOS image targets don't load) |

## Round 1 (2026-11-06)

- Implementer: 00eeaecb, tasks 19/19, checks clippy/machete/Bazel ok, container tests ran
- Reviewed range: BASE...00eeaecb
- Security: 0 findings ≥ 8 (no verify step needed); one sub-threshold note recorded as D1.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S2, P1 | Standards, Spec | P1 | `crates/df-k8s/README.md:63-64` | Cluster target bullets still say a failing rule without cache is skipped / run starts without the target | fix-now | fixed (fe2dda11) |
| S1, P2 | Standards, Spec | P1 | `crates/df-k8s/README.md:86-91` | RBAC list misses `events` get/list/watch | fix-now | fixed (fe2dda11) |
| P5 | Spec | P1 | `docs/content/operations/troubleshooting.md:17` | Only the Event-path detail is documented; container-status path gives `ErrImagePull: …` / `ImagePullBackOff: …` | fix-now | fixed (fe2dda11) |
| P6 | Spec | P1 | `docs/content/operations/troubleshooting.md:14` | 403 is permanent, read-access hint wrong; named port without ready endpoint missing | fix-now | fixed (fe2dda11) |
| P3 | Spec | P1 | `crates/df-k8s/src/provider/pull_events.rs:3-4` | Module doc contradicts task 1.1 finding | fix-now | fixed (fe2dda11) |
| P4 | Spec | P1 | `crates/df-k8s/src/provider.rs:846-849` | `EventTracker` doc points to timeouts instead of the startup sweep | fix-now | fixed (fe2dda11) |
| P7 | Spec | P1 | `crates/df-k8s/src/reconcile/tests.rs:900` | Reconcile-path warning naming the target is not asserted | fix-now | fixed (fe2dda11) |
| S3 | Standards | P1 | `crates/dark-factory/src/sandbox.rs:208-220` | Unavailable error built in the binary from df-k8s data; message literal copied into 3 fakes | fix-now | fixed (fe2dda11) |
| S4 | Standards | P1 | `crates/df-agent-channel/src/scheduler.rs:64,95-98` | Sweep runs in the `provisions` JoinSet; shutdown warning names provisioning only | fix-now | fixed (fe2dda11) |
| S6 | Standards | P1 | `crates/dark-factory/src/sandbox.rs:484-492` | Test inserts allowlist rows with raw SQL instead of `df_db::egress::add_entry` | fix-now | fixed (fe2dda11) |
| P9 | Spec | P1 | `tasks.md` 1.5 | Task text does not match what was done (design/deployment.md, rbac_test.yaml) | fix-now | fixed (fe2dda11) |
| P10 | Spec | P1 | `design.md` D5 | Extra `DF_LIVE_*` variables not named in D5 | fix-now | fixed (fe2dda11) |
| D1 | Security (sub-threshold) | P2 | `crates/df-k8s/src/reconcile.rs:258-267` | While one rule is transient without cache the whole policy is kept, so removing another rule does not reach running sandboxes | tech-debt | tech-debt (TD-121) |
| S5 | Standards | P3 | `crates/df-agent-channel/src/scheduler.rs:149-176` | Same three-arm match on `sandbox_exists` twice | cosmetic | cosmetic |
| S7 | Standards | P3 | `crates/df-k8s/src/provider/pull_events.rs:24` | `pub` unreachable outside the crate; `Option<&Pod>` parameter | cosmetic | cosmetic |
| S8 | Standards | P3 | `crates/df-k8s/src/provider.rs:935-981` | `Watched::Event` / `pod_events` ambiguous names | cosmetic | cosmetic |
| S9 | Standards | P3 | `crates/df-k8s/src/test_support.rs` | Needless `pub(crate)`/`Clone`; literals instead of constants in tests | cosmetic | cosmetic |
| S10 | Standards | P3 | `crates/df-k8s/src/provider/tests/pull_events.rs:17-27` | Run-label fixture duplicated | cosmetic | cosmetic |
| P8 | Spec | P3 | `tasks.md` 1.6 | Live check used the runtime image volume instead of the harness (recorded) | accepted | cosmetic |

User decision: Fix blocking + P1 (no blocking findings; D1 to tech debt, P3 ledger only).

## Round 2 (2026-11-06)

- Implementer: fe2dda11 (fix round 1), checks clippy/fmt/machete ok, df-k8s/dark-factory/df-agent-channel tests ok, Bazel 6/6, docs build ok, container tests ran
- Reviewed range: 00eeaecb...fe2dda11
- Ledger check: all round 1 fix-now items confirmed fixed. Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-P1 | Spec | P1 | `docs/content/operations/troubleshooting.md:14`, `crates/df-k8s/README.md` | "named `targetPort` with no ready endpoint" is wrong: the error comes only when no EndpointSlice port has that name (readiness is not checked) | fix-now | fixed (5bb0a934) |
| R2-S1 | Standards | P1 | `crates/df-k8s/README.md:62` | The removed bullet dropped the database-error case (provisioning fails) | fix-now | fixed (5bb0a934) |
| R2-P2 | Spec | P1 | `design.md` D4, `tasks.md` 3.1 | Still name the JoinSet `provisions` (now `background`) | fix-now | fixed (5bb0a934) |
| R2-S2 | Standards | P1 | `crates/df-k8s/src/provider/tests/lifecycle.rs:439`, `crates/df-k8s/src/reconcile/tests.rs:32` | Same 10-line unavailable fixture in two fakes | fix-now | fixed (5bb0a934) |
| R2-S3 | Standards | P1 | `crates/df-k8s/src/reconcile/tests.rs:907` | Test name does not cover the new warning assertion | fix-now | fixed (5bb0a934) |

User decision: Fix blocking + P1 (first answer "Fix blocking only" was a wrong click, corrected by the user).

## Round 3 (2026-11-06)

- Implementer: 5bb0a934 (fix round 2), checks clippy/fmt/machete ok, df-k8s tests ok, Bazel df-k8s ok, docs build ok
- Reviewed range: fe2dda11...5bb0a934
- Ledger check: all round 2 items confirmed fixed. Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R3-P1 | Spec | P1 | `docs/content/operations/troubleshooting.md:14`, `crates/df-k8s/README.md:60` | The named-port transient case also covers selected pods that don't declare the named container port; docs point only to missing pods | fix-now | fixed (4295ff0b) |
| R3-S1 | Standards | P3 | `crates/df-k8s/src/test_support.rs:53` | Fixture placed inside the live-config block; module doc mentions only live tests | fix-now | fixed (4295ff0b) |

User decision: Fix blocking + P1 (R3-S1 included, it is a one-line move).

## Round 4 (2026-11-06)

- Implementer: 4295ff0b (fix round 3), checks clippy/fmt df-k8s ok, df-k8s tests ok, docs build ok
- Reviewed range: 5bb0a934...4295ff0b (Standards and Spec; Security skipped: docs and test-only code, excluded by its rules)
- Ledger check: R3-P1 and R3-S1 confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R4-S1 | Standards | P3 | `crates/df-k8s/src/test_support.rs:131` | `pub(crate)` fixture after the private `service_ip` | fix-now (orchestrator moved it) | fixed (5b6e1833) |

User decision: move it directly, then finish (no further review round).

## Round 5 (2026-11-07)

- Implementer: 5b6e1833 (fix round 4), checks clippy/fmt/tests df-k8s ok; Bazel build `--lockfile_mode=error` ok
- Reviewed range: ab992ff3...5b6e1833 (whole change, at the user's request after round 4; all three axes)
- Ledger check: all earlier fixed rows still hold. Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R5-P1 | Spec | P1 | `crates/df-agent-channel/docs/configuration.md:85-110` | Run state diagram lacks `provisioning → failed: connect_timeout, sandbox_lost` and `connecting → failed: sandbox_lost`; the startup sweep can end any non-terminal state with `sandbox_lost` | fix-now | fixed (20e55aa6) |
| R5-P2 | Spec | P1 | `docs/content/operations/troubleshooting.md:14` | Documented warning is logged only on the first attempt; retries log `provisioning a stale run again failed; retrying on a later tick` | fix-now | fixed (20e55aa6) |
| R5-P4 | Spec | P1 | `tasks.md:46`, `proposal.md:40` | TD-29 deleted but not named in task 5.2 (text and grep) and the proposal's Impact | fix-now | fixed (20e55aa6) |
| R5-S1 | Standards | P1 | `crates/df-agent-channel/src/scheduler/tests.rs:551,681` | Teardowns counted inline although the `teardowns(&env)` helper exists | fix-now | fixed (20e55aa6) |
| R5-S2 | Standards | P1 | `crates/df-k8s/src/provider/tests/lifecycle.rs:444` | Test repeats the body of `mock_provider` with another targets provider | fix-now | fixed (20e55aa6) |
| R5-P3 | Spec | – | `docs/tech_debt.md` | No TD entry for D1 yet | finish step 4 | tech-debt (TD-121) |
| R5-S3 | Standards | P3 | `crates/df-agent-channel/src/scheduler.rs:147`, `runs.rs:141` | "Run has no sandbox yet" rule defined in Rust and in SQL | cosmetic | cosmetic |

User decision: Fix blocking + P1.

## Round 6 (2026-11-07)

- Implementer: 20e55aa6 (fix round 5), checks fmt/clippy/machete ok, df-agent-channel and df-k8s tests ok (container socket available), docs build ok, openspec validate ok
- Reviewed range: 5b6e1833...20e55aa6 (Standards and Spec; Security skipped: docs and test-only code)
- Ledger check: R5-P1, R5-P2, R5-P4, R5-S1, R5-S2 confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R6-P1 | Spec | Blocking (minor, in code the last round changed) | `crates/df-agent-channel/docs/configuration.md:94-103` | Diagram lacks `awaiting_input --> failed: sandbox_lost` and `finalizing --> failed: sandbox_lost`; the sentence below says the sweep ends any state from `connecting` on | fix-now (orchestrator) | fixed (2cafe680) |
| R6-P2 | Spec | Blocking (nit, same diagram) | `crates/df-agent-channel/docs/configuration.md:88` | `enforce_timeouts` also ends `pending` with `connect_timeout`; edge missing | fix-now (orchestrator) | fixed (2cafe680) |
| R6-S1 | Standards | Blocking (nit, same diagram) | `crates/df-agent-channel/docs/configuration.md:91` | `provisioning` edge joins two outcomes with a comma, `connecting` uses one edge per outcome | fix-now (orchestrator) | fixed (2cafe680) |

User decision: fix directly (orchestrator), then a short Spec re-check.

## Round 7 (2026-11-07)

- Fix: 2cafe680 (fix round 6, diagram only), docs build and link test ok
- Reviewed range: 20e55aa6...2cafe680 (Spec only, per the user's decision)
- Ledger check: R6-P1, R6-P2, R6-S1 confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R7-P1 | Spec | P3 (pre-existing behavior, not this change) | `crates/df-agent-channel/docs/configuration.md:101,106` | Hard stop also ends `preparing`/`awaiting_input`/`finalizing` with `failed`; only `running` shows it | cosmetic | cosmetic |

User decision: leave R7-P1 as P3, then finish.
