# Review log: iter2-run-resilience

- BASE: 715e2e8b08c0ba17500a7bd30b6cc2aeb1d365a0
- Store: none
- Status: done
- Implementer agent: a1037d9f98d906a45
- DOCKER_HOST: unix:///run/user/501/podman/podman.sock
- Design: 587683b (boards Desktop/Phone · rate limited, resumes; approved 2026-11-02)
- Step: 7 (finished)

## Round 1 (2026-11-02)

- Implementer: faef584, tasks 12/12, checks fmt/clippy/machete/tests ok, .sqlx regenerated, Bazel lockfile unchanged (no Cargo changes), container tests ran (podman); e2e `rate_limited_task_resumes_at_the_retry_time_without_a_message` is `#[ignore]` (kind cluster), not run
- Reviewed range: 715e2e8...faef584

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1+P2 | Spec | Blocking | `crates/df-agent-channel/src/scheduler.rs:107-158`, `runs.rs:130` | Stale runs re-provisioned every tick even with a sandbox; retry error ends healthy run `provisioning_failed`; "connects normally" untested | fix-now | fixed (e6e6f35) |
| P3 | Spec | Blocking | `crates/factory-agent/src/connection.rs:5`, `tests/connection.rs` | Flush scenarios lack manager-level tests (60 s outage, >10 min exit); `std::time::Instant` blocks paused time | fix-now | fixed (e6e6f35) |
| S2+S3 | Standards | P1 | `runs.rs:162,257-300`, `session.rs:986` | Config cap on `RunEnd`; count reset rule split | fix-now | fixed (e6e6f35) |
| P4 | Spec | P1 | `crates/df-web/src/task_pages/tests/resume.rs:46` | Rendered resume time not asserted | fix-now | fixed (e6e6f35) |
| P7 | Spec | P1 | `crates/df-core/src/config.rs:515` | `maxConcurrent: 0` accepted | fix-now | fixed (e6e6f35) |
| S1 | Standards | P1 | `runs.rs:726` | Inline test module ~556 lines | fix-now | fixed (e6e6f35) |
| S5 | Standards | P1 | `view.rs:183`, `resume.rs`, `runs.rs:127` | "awaits resume" status set repeated | fix-now | fixed (e6e6f35) |
| S6 | Standards | P1 | `resume.rs:1510` | Outcome strings re-matched | fix-now | fixed (e6e6f35) |
| S8 | Standards | P1 | `runs.rs:312` | Test named after design doc | fix-now | fixed (e6e6f35) |
| S9 | Standards | P1 | `factory-agent tests/connection.rs` | Constant-detector test | fix-now | fixed (e6e6f35) |
| S11 | Standards | P1 | `view.rs:580` | Long chain inside `html!` | fix-now | fixed (e6e6f35) |
| S7 | Standards | P3→fix | `scheduler/tests.rs:12` | `tick` helper always `Ok` | fix-now (user) | fixed (e6e6f35) |
| S10 | Standards | P3→fix | `view.rs:547` | Now branches duplicate markup | fix-now (user) | fixed (e6e6f35) |
| S12 | Standards | P3→fix | `scheduler/tests.rs:829` | Duplicated test SQL helper | fix-now (user) | fixed (e6e6f35) |
| S4 | Standards | P3 | `domain.rs:1063` | `"capacity"` string marker in detail (spec prescribes detail `capacity`) | cosmetic | cosmetic |
| P5 | Spec | Rejected | `view.rs:445` | Task row subtitle / reason label as scope creep | — | rejected (on approved PenPot boards) |
| P6 | Spec | Rejected | `crates/df-e2e/tests/flow.rs:186` | Third run in e2e | — | rejected (message sent seconds after rate_limited clears the 60 s resume time) |
| X | Security | — | — | No findings | — | — |

User decision: Fix blocking + P1, plus S7, S10, S12.

## Round 2 (2026-11-02)

- Implementer: e6e6f35, tasks 12/12, checks fmt/clippy/machete/tests ok, MODULE.bazel.lock regenerated (tokio test-util dev-dep for factory-agent), container tests ran (podman)
- Reviewed range: faef584...e6e6f35

Ledger check: all 14 fix-now rows fixed (e6e6f35), no regressions.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P8+S15 | Spec/Standards | P1 | `crates/df-k8s/src/provider.rs:723` | `K8sSandboxProvider::sandbox_exists` untested; wrong selector would silently re-enable re-provisioning | fix-now | fixed (b801920) |
| S14 | Standards | P1 | `crates/df-k8s/src/provider.rs:683-692,724-728` | Selector + list duplicated from `teardown` | fix-now | fixed (b801920) |
| S13 | Standards | P1 | `crates/df-web/src/task_pages/view.rs:183` | Longer chain in `task_rows`, status parsed twice | fix-now | fixed (b801920) |
| S17 | Standards | P1 | `crates/df-agent-channel/src/testing.rs:321` | `task_resume_state` returns positional tuple | fix-now | fixed (b801920) |
| P9 | Spec | P1 | `crates/df-web/src/task_pages/tests/resume.rs:12` | Test flaky across UTC midnight | fix-now | fixed (b801920) |
| P10 | Spec | P1 | `crates/factory-agent/src/tests/connection.rs` | `finalize_during_an_outage` doc comment mismatches 60 s test | fix-now | fixed (b801920) |
| S16 | Standards | P3 | `crates/df-core/src/config.rs:28,73` | `NonZeroU32` instead of hand check (trade-off: weaker error message) | cosmetic | cosmetic |
| X | Security | — | — | No findings | — | — |

User decision: Fix blocking + P1.

## Round 3 (2026-11-02)

- Implementer: b801920, tasks 12/12, checks fmt/clippy/machete/tests ok, container tests ran (podman)
- Reviewed range: e6e6f35...b801920

Ledger check: all 6 round-2 fix-now rows fixed (b801920).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S19 | Standards | P1 | `crates/df-k8s/src/provider.rs:556` | Private `list_run_sandboxes` between public methods | fix-now | fixed (7876008) |
| S20 | Standards | P1 | `crates/df-k8s/src/provider.rs:557,733` | Helper returns `(Api, ObjectList)` tuple, half discarded | fix-now | fixed (7876008) |
| S21 | Standards | P1 | `crates/df-agent-channel/src/testing.rs:320` | `task_resume_state` lost its doc | fix-now | fixed (7876008) |
| S22 | Standards | P1 | `crates/df-agent-channel/src/runs/tests.rs:268…` | `auto_resumes: count` rename + repeated destructure | fix-now | fixed (7876008) |
| S18 | Standards | P2→fix | `crates/df-k8s/src/provider/tests.rs:1124` | Mock provider setup duplicated 4× | fix-now (user) | fixed (7876008) |
| X | Security | — | — | No findings | — | — |
| P | Spec | — | — | No findings | — | — |

User decision: Fix blocking + P1, plus S18.

## Round 4 (2026-11-02)

- Implementer: 7876008, tasks 12/12, checks fmt/clippy/machete ok, df-k8s + df-agent-channel tests ran (podman); df-web and factory-agent not re-run (unchanged)
- Reviewed range: b801920...7876008

Ledger check: S19–S22 fixed (7876008); S18 fixed except one copy (→ S23).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S23 | Standards | P1 | `crates/df-k8s/src/provider/tests.rs:645` | Last copy of the mock provider setup (rest of S18) | fix-now | fixed (f8b28c0, 2ab1511) |
| S24 | Standards | P3→fix | `crates/df-k8s/src/provider.rs:645` | `provision` builds `Api<Sandbox>` inline instead of `sandbox_api()` | fix-now (user) | fixed (f8b28c0, 2ab1511) |
| S25 | Standards | P3→fix | `crates/df-k8s/src/provider/tests.rs:785,1126` | Duplicated Status error-body JSON | fix-now (user) | fixed (f8b28c0, 2ab1511) |
| X | Security | — | — | No findings | — | — |
| P | Spec | — | — | No findings (pure refactor) | — | — |

User decision: Fix S23–S25; orchestrator verifies the diff instead of a full review round, then finish.

## Round 5 (2026-11-02)

- Implementer: f8b28c0 + fixup 2ab1511 (doc comment of `send_json`), df-k8s tests, fmt, clippy ok
- Verified by the orchestrator (diff read), no reviewer round per user decision. S23, S24, S25 fixed.

## Finish

- Container tests: ran with podman in every round (df-test-postgres). Not run: `crates/df-e2e` test `rate_limited_task_resumes_at_the_retry_time_without_a_message` (`#[ignore]`, needs the kind cluster of `just e2e-local`).
- Bazel: `bazel --bazelrc=.bazelrc.agents build --lockfile_mode=error //crates/... //plugins/...` passed (after 7876008; later rounds touched no Cargo files).
- Tech debt: none (S18, the only P2, was fixed on user request).
- Roadmap: backlog row "Priorities between due tasks in the scheduler".
- Cosmetic, not fixed: S4, S16. Rejected: P5, P6.
