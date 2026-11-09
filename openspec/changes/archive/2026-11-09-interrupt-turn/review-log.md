# Review log: interrupt-turn

- BASE: daf0f4765054a98b4312f50086d2e8c504939f0a
- Store: none
- Status: untested
- Implementer agent: a237e8aa612b6cca2

## Round 1 (2026-11-09)

- Implementer: f3b3808, tasks 13/14 (5.1 e2e written, not run: kind cluster image pulls fail on sandbox TLS proxy), checks: fmt, clippy, machete, xtask checks, docs-build, openspec validate, cargo tests per crate, bazel test //... 52/52 all pass; container tests ran (docker.sock)
- Reviewed range: BASE...f3b3808

Security: 0 findings (no verify step needed).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `deploy/e2e/e2e-local.sh` | Execute bit lost (100755→100644); `just e2e-local` and CI e2e fail | fix-now | fixed (b3c0a80) |
| P2 | Spec | Blocking | `crates/factory-agent/src/runner.rs:1082` | Race: interrupt receiver subscribed in `await_prompt` after `begin_turn` sent TurnStarted; an Interrupt in between is dropped | fix-now | fixed (b3c0a80) |
| P4 | Spec | Blocking | `crates/df-e2e/tests/interrupt.rs` | Task 5.1 e2e never run; held-back-message-after-interrupt scenario unverified on the control-plane side | fix-now | open |
| P5 | Spec | Blocking | `crates/df-agent-channel/src/session/tests.rs` | Scenario "Runtime not connected" has no test | fix-now | fixed (b3c0a80) |
| P3 | Spec | P1 | `crates/factory-agent/src/runner.rs:1189`, `docs/content/using/tasks.md:96` | Cancel timeout/error path: TurnEnded ERROR still carries `interrupted_by`, run fails; docs say flatly "no failure" | fix-now | fixed (b3c0a80) |
| P6 | Spec | P1 | `crates/df-db/src/tasks/interrupt_tests.rs:113` | Test doesn't assert task status unchanged | fix-now | fixed (b3c0a80) |
| S2 | Standards | P1 | `crates/df-web/src/task_pages/view.rs:587,829` | "Can stop" predicate duplicated | fix-now | fixed (b3c0a80) |
| S3 | Standards | P1 | `crates/df-web/src/components/transcript.rs:262` | `forge_row` doc says forge-only, now used for interrupt row | fix-now | fixed (b3c0a80) |
| P7 | Spec | P3 | `crates/factory-agent/src/tests/runner_idle.rs:668` | Budget test uses max_turns 1, not 3 | cosmetic | cosmetic |
| S1 | Standards | P3 | `crates/factory-agent/src/runner.rs:388` | `recv_cancel` now also receives interrupts; rename | fix-now | fixed (b3c0a80) |
| S4 | Standards | P3 | `crates/factory-agent/src/transcript.rs:333` | `""` encodes "not interrupted"; take `Option<&str>` | fix-now | fixed (b3c0a80) |
| S5 | Standards | P3 | `crates/df-agent-channel/src/session.rs:415` | `interrupt_on` name unclear | fix-now | fixed (b3c0a80) |
| S6 | Standards | P3 | `crates/factory-agent/src/runner.rs:1156` | Fully qualified inline proto construction | fix-now | fixed (b3c0a80) |

User decision: Adjust: fix blocking + P1 + S1, S4, S5, S6 (P7 stays cosmetic).

## Round 2 (2026-11-09)

- Implementer: b3c0a80, tasks 13/14 (5.1 still not run: with E2E_KIND_CONFIG CA mount + NODE_EXTRA_CA_CERTS pulls work, but in-cluster DNS fails in the nested kind node: CoreDNS "Plugins not ready: kubernetes", migrate job can't resolve postgres.postgres.svc). Checks all pass, container tests ran, bazel test 52/52.
- Reviewed range: f3b3808...b3c0a80

Security: 0 findings. Ledger check: all items marked fixed (b3c0a80) confirmed by the standards and spec reviewers; P4 still open.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P4 | Spec | Blocking | `crates/df-e2e/tests/interrupt.rs` | (carried over) e2e never run; nested kind cluster DNS fails in sandbox | fix-now | open |
| R2-P2T | Spec | Blocking | `crates/factory-agent/src/tests/runner_idle.rs:645` | P2 regression test likely passes without the fix (no yield between start events and the old subscribe on a current-thread runtime) | fix-now | fixed (134bb3c) |
| R2-S1 | Standards | P1 | `crates/factory-agent/src/runner.rs:1398` | `text_block` helper in runner.rs while transcript.rs builds the same block by hand 4× | fix-now | fixed (134bb3c) |
| R2-P3T | Spec | P1 | `crates/factory-agent/src/runner.rs:1137` | Cancel-timeout path (no answer within 5 s) has no own test | fix-now | fixed (134bb3c) |
| R2-S2 | Standards | P3 | `crates/df-web/src/components/transcript.rs:262` | `forge_row` name still forge-only; doc line not wrapped | fix-now | fixed (134bb3c) |
| R2-S3 | Standards | P3 | `crates/df-db/src/tasks/interrupt_tests.rs:48,59` | `tests::status_of` qualified while other helpers are imported | fix-now | fixed (134bb3c) |
| R2-S4 | Standards | P3 | `docs/content/using/tasks.md:96` | Long parenthetical; new doc lines not wrapped like the rest | fix-now | fixed (134bb3c) |
| R2-KIND | Spec | Rejected | `deploy/e2e/e2e-local.sh` | `E2E_KIND_CONFIG` opt-in outside the spec | rejected (justified dev tooling, default unchanged, documented) | rejected |
| R2-HTC | Spec | Rejected | `crates/fake-acp-agent/src/script.rs:193` | `hang_then_crash` step outside the spec | rejected (test support for P3) | rejected |

User decision: Adjust: fix blocking + P1 + R2-S2, R2-S3, R2-S4 (all open items). P4: one more bounded e2e attempt.

## Round 3 (2026-11-09)

- Implementer: 134bb3c, tasks 13/14 (5.1 not run: kube-dns Service VIP 10.96.0.10 answers NXDOMAIN for cluster names inside the nested kind node; CoreDNS pod IPs resolve fine; no iptables rules for the VIP). Old P2 test confirmed to pass without the fix (3/3), replaced; new test fails on the old ordering. Checks all pass, container tests ran, bazel test 52/52.
- Reviewed range: b3c0a80...134bb3c

Security: 0 findings. Spec: 0 new findings; R2-P2T and R2-P3T confirmed fixed; P4 unchanged.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P4 | Spec | Blocking | `crates/df-e2e/tests/interrupt.rs` | (carried over) e2e never run; sandbox kind DNS VIP broken | finish untested | open |
| R3-S1 | Standards | Blocking | `crates/factory-agent/src/tests/runner_idle.rs:724` | Cancel-timeout test polls with real 20 ms sleeps against a paused clock in an unbounded loop; a regression hangs CI instead of failing | fix-now | fixed (99eea17) |
| R3-S4 | Standards | P1 | `docs/design-system.md:179,187` | Contributor doc still names `forge_row` (rename to `system_row`, mention the interrupt row) | fix-now | fixed (99eea17) |
| R3-S2 | Standards | P3 | `crates/factory-agent/src/tests/runner_idle.rs:669` | Magic 50 yields; name it or explain | cosmetic | cosmetic |
| R3-S3 | Standards | P3 | `crates/factory-agent/src/tests/runner_idle.rs:733` | `ended \|= turn_endings(slice::from_ref(..))` hard to read | cosmetic | cosmetic |

User decision: Fix blocking + P1 (R3-S1, R3-S4). R3-S2/S3 cosmetic. P4 cannot be fixed in this sandbox.

## Round 4 (2026-11-09)

- Implementer: 99eea17, tasks 13/14 (5.1 not retried). `Timeouts { cancel_response, harness_exit_grace }` on `RunDriverParams` (defaults 5 s, test hook only). Checks all pass, bazel test 52/52.
- Reviewed range: 134bb3c...99eea17

Security: 0 findings. Spec: 0 new findings; R3-S1 and R3-S4 confirmed fixed; P4 unchanged.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P4 | Spec | Blocking | `crates/df-e2e/tests/interrupt.rs` | (carried over) e2e never run; sandbox kind DNS VIP broken | finish untested | open |
| R4-S1 | Standards | P3 | `crates/factory-agent/src/tests/runner_runtime.rs:238…`, `tests/runner.rs:534` | `crate::runner::Timeouts::default()` qualified where `crate::runner::{…}` is already imported locally | cosmetic | cosmetic |

User decision: finish untested (fix-round limit reached). R4-S1 cosmetic.

## Finish (2026-11-09)

- Status: untested. Not run: `crates/df-e2e/tests/interrupt.rs` via `just e2e-local` (task 5.1): the nested kind cluster in the sandbox resolves no cluster names through the kube-dns Service VIP. All other container-backed tests (df-db, df-agent-channel, df-web via df_test_postgres) ran against `/var/run/docker.sock`; `bazel test //...` 52/52.
- Open: P4 (e2e not run).
- Bazel build with `--lockfile_mode=error`: not needed (no Cargo.toml/Cargo.lock change).
- Docs: `just docs-build` passed. Changed pages: `docs/content/using/tasks.md`, `docs/content/architecture/agent-runs.md`, `crates/factory-agent/docs/configuration.md`; contributor docs `docs/demo.md`, `docs/design-system.md`.
- Tech debt: none (no P2 findings).
- Roadmap: backlog row "Interrupt the current turn" → implemented; new row "Send and interrupt" on a held-back message.
