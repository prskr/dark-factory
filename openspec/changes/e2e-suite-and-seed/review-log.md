# Review log: e2e-suite-and-seed

- BASE: 3f6a5eb996ab010ca51b516f052bb4bdddf38082
- Store: none
- Status: done
- Implementer agent: aaf4e0bf77e7200ca

## Round 1 (2026-11-13)

- Implementer: 7799131, tasks 21/21, checks fmt/clippy/machete/xtask/shellcheck/helm-unittest/openspec --strict/docs-build ok, cargo tests ok, e2e full suite green (1 thread, 12m35s), container tests ran (none skipped)
- Implementer notes: default `E2E_TEST_THREADS` kept at 1 (Playwright WebSocket breaks with 2), spec requires 2; intermittent `coordinator_delegates_…` failure (DG-3) not root-caused; demo profile has no `running` task (replaced by `waiting`)
- Reviewed range: BASE...7799131
- Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `deploy/e2e/e2e-local.sh:558` | `E2E_TEST_THREADS` default 1, spec requires 2; design D4 edit changes the decision (peak 2.1 GiB, kept 1 for Playwright WS flakiness) | fix WS root cause, default 2 | fixed (195c634) |
| P3 | Spec | Blocking | `crates/df-e2e/src/seed/demo/write.rs:225` | completed demo tasks never set `pr_merged_at`, so the "merged PR" state is missing; demo_seed test doesn't assert it (verified) | fix-now | fixed (195c634) |
| P4 | Spec | Blocking | `crates/df-agent-channel/docs/configuration.md:38` | paragraph inside the table ends it; later rows (`artifacts.*`, `web.*`, …) render as text (verified) | fix-now | fixed (195c634) |
| P5 | Spec | Blocking | `deploy/e2e/e2e-local.sh:84` | skipped binaries printed only with `E2E_LIST=1`; scenario "Excluded binary" requires it on a normal run | fix-now | fixed (195c634) |
| S1 | Standards | P1 | `crates/df-e2e/src/seed.rs:34` | `#[expect(too_many_lines)]` added instead of splitting; move project list to const | fix-now | fixed (195c634) |
| S2 | Standards | P1 | `deploy/helm/dark-factory/tests/instance_config_test.yaml:324` | test name claims unknown keys, only checks tick 0 | fix-now | fixed (195c634) |
| S3 | Standards | P1 | `crates/df-core/src/config.rs:792` | `TimeDelta::seconds` can panic if `validate` skipped (pub fields); clamp or `try_seconds` | fix-now | fixed (195c634) |
| S4 | Standards | P1 | `pr/mark_draft.rs:20`, `pr/sync.rs:31` | duplicated `retry_policy` + tests | fix-now | fixed (195c634) |
| S7 | Standards | P1 | `df-db/src/outbox/dispatcher/tests.rs:410`, `scheduler/tests.rs:1358` | polling loop without timeout can hang | fix-now | fixed (195c634) |
| S8 | Standards | P1 | `deploy/e2e/e2e-local.sh:316` | failed `forward` masked by assignment | fix-now | fixed (195c634) |
| S9 | Standards | P1 | `deploy/e2e/e2e-local.sh:415` | repo DELETE `|| true` swallows non-404 errors | fix-now | fixed (195c634) |
| S10 | Standards | P1 | `deploy/e2e/e2e-local.sh:344` | missing `e2e-state` key yields empty token silently | fix-now | fixed (195c634) |
| P7 | Spec | P1 | `docs/demo.md`, design D4 | inconsistent 2-thread numbers (6 of 6 vs 4 of 6); 1-thread flake not stated as known issue | fix-now | fixed (195c634) |
| F1 | Implementer | P2 → fix | `crates/df-e2e/tests/delegation.rs` | `coordinator_delegates_…` intermittently fails (`closed connection` at DG-3); same root cause as P1 | fix-now | fixed (195c634) |
| S5 | Standards | P3 | `seed/demo/data.rs`, `write.rs` | outcome/status as `&'static str` instead of enums (D7 test catches typos) | fix-now (user) | fixed (195c634) |
| S6 | Standards | P3 | `seed.rs:125`, `demo.rs:169`, `write.rs` | duplicated `ProjectInput`/PR URL building | fix-now (user) | fixed (195c634) |
| S11 | Standards | P3 | `deploy/e2e/e2e-local.sh:168` | empty `df` output → arithmetic error | fix-now (user) | fixed (195c634) |
| S12 | Standards | P3 | `deploy/e2e/e2e-local.sh:424` | repeated rollout lines | fix-now (user) | fixed (195c634) |
| P6 | Spec | P3 | runs.rs wiring | no channel-level test of configured backoff | fix-now (user) | fixed (195c634) |
| P8 | Spec | P3 | `tests/lifecycle_events.rs:75` etc. | small out-of-scope robustness fixes; list in summary | fix-now (user) | fixed (195c634, commit body) |
| P2 | Spec | P3 (artifact) | `specs/demo-data/spec.md` | spec should say queued/rate-limited demo tasks have future resume times; needs /opsx:update | deferred (user, /opsx:update later) | open |
| P9 | Spec | P3 (artifact) | `proposal.md` | "e2e seed stays as it is" vs new project `RT`; needs /opsx:update | deferred (user, /opsx:update later) | open |

User decision (round 1): fix everything except P2 and P9 (deferred; the user addresses them later via /opsx:update). P1: fix the Playwright WebSocket drop and set the default to 2 threads; the spec stays as is. F1 is in the fix round (likely the same root cause).

Fix round 1: 195c634. P1/F1 root cause: orphaned `kubectl port-forward` (backgrounded shell function, `$!` = subshell) held 13001 pointing at the old Playwright pod; fixed, default 2 threads, 3 consecutive green full runs (8m31s/7m27s/7m28s, peak 3.41-3.47 GiB). Checks green, container tests ran.

## Round 2 (2026-11-13)

- Reviewed range: 7799131...195c634 plus ledger check
- Ledger check: all round-1 fix-now items fixed. Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S13 | Standards | Blocking | `deploy/e2e/e2e-local.sh:455` | port-in-use failure inside `cluster_is_reusable` is silenced and treated as "not reusable", so a kept cluster is deleted and the later forward fails on the same port anyway (verified) | fix-now | fixed (2c6fa4d) |
| S14 | Standards | Blocking | `deploy/e2e/e2e-local.sh:303` | `forward` failure path leaves kubectl running; not killed by `stop_forgejo_forward` (verified) | fix-now | fixed (2c6fa4d) |
| S16 | Standards | P1 | `crates/df-e2e/src/seed/demo/write.rs:315` | outcome→run state mapping copied as strings from `runs::run_state_for` | fix-now | fixed (2c6fa4d) |
| S15 | Standards | P1 | `deploy/e2e/e2e-local.sh:295` | hint `pkill -x kubectl` kills unrelated kubectl processes | fix-now | fixed (2c6fa4d) |
| S17 | Standards | P1 | `crates/df-core/src/config.rs:795` | `unwrap_or(0)` after clamp is a misleading fallback (0 = resume immediately) | fix-now | fixed (2c6fa4d) |
| P10 | Spec | P1 | `docs/demo.md:158` | docs say only `E2E_LIST=1` prints skipped binaries | fix-now | fixed (2c6fa4d) |
| P11 | Spec | P1 | `docs/demo.md:303` | "the two rows above" no longer points at the 1-thread rows; lost qualifiers | fix-now | fixed (2c6fa4d) |
| S18 | Standards | P3 | `deploy/e2e/e2e-local.sh:431` | DELETE curl repeats `forgejo_api` URL/auth | — | cosmetic |
| S19 | Standards | P3 | `crates/df-e2e/src/seed.rs:351` | 7 positional args in `project_input` | — | cosmetic |

User decision (round 2): fix blocking + P1 (S13, S14, S15, S16, S17, P10, P11); S18, S19 cosmetic.

Fix round 2: 2c6fa4d. S13 verified by implementer (held port → exit with message, kept cluster survives; targeted reuse run passed). Checks green.

## Round 3 (2026-11-13)

- Reviewed range: 195c634...2c6fa4d plus ledger check
- Ledger check: S13-S17, P10, P11 fixed. Security: 0 findings. No blocking findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S20 | Standards | P1 | `crates/df-core/src/domain.rs:448` | free `run_state_for` between `impl RunOutcome` blocks; make it a method | fix-now | fixed (18a903a) |
| S21 | Standards | P1 | `crates/df-agent-channel/src/runs/tests.rs:19` | mapping test stayed in channel crate; table test over all outcomes in df-core | fix-now | fixed (18a903a) |
| P12 | Spec | P1 | `docs/demo.md:37,54` | step 0 and flowchart don't mention the up-front port check | fix-now | fixed (18a903a) |
| P14 | Spec | P1 | `deploy/e2e/e2e-local.sh:117` | `install_traps` comment names only the disk check | fix-now | fixed (18a903a) |
| S22 | Standards | P3 | `deploy/e2e/e2e-local.sh:315` | port probe and hint duplicated in `check_ports_free` and `forward` | — | cosmetic |
| P13 | Spec | P3 | `deploy/e2e/e2e-local.sh:317` | up-front port check not named in design D4 | — | cosmetic |

User decision (round 3): fix P1 (S20, S21, P12, P14), then finish; S22, P13 cosmetic.

Fix round 3: 18a903a, checked by the orchestrator (small diff: method move, table test, comment and docs). Checks green per implementer.

## Finish (2026-11-13)

- Container-backed tests: all ran against /var/run/docker.sock in every round; none skipped.
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` passed (Cargo.lock, MODULE.bazel.lock, df-e2e Cargo.toml changed).
- Docs: `just docs-build` passed. Published pages changed: `crates/df-agent-channel/docs/configuration.md` (reference/agent-channel, `scheduling.*`), `docs/content/architecture/agent-runs.md`, `docs/content/operations/troubleshooting.md`. Contributor docs: `docs/demo.md`, `docs/releasing.md`, `docs/development.md`, `docs/tech_debt.md` (TD-123 removed), `.agents/skills/rust/SKILL.md`, Helm chart README.
- Tech debt: none created.
- Roadmap: 2 backlog rows added (automated screenshot capture; compile-time measurement / debug info / nextest), "Showcase seed" row narrowed to the landing-page screenshots.
- Open (deferred by the user, not blocking): P2, P9 via /opsx:update.
