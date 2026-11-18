# Review log: orchestrator-crate

- BASE: f6c17bbf458847e0067c73434233b9614f17728a
- Store: none
- Status: done
- Implementer agent: a4546dd2492ac768d

## Round 1 (2026-11-18)

- Implementer: 79743aad + 64b6de9a, tasks 9/9, checks: fmt, clippy, machete, check-crates/file-length/test-names, docs-build, cargo tests (channel 230 + orchestrator 138 = 368, before 367 + 1 new), dark-factory 69, Bazel build and test, e2e-local all pass; container tests ran
- Reviewed range: BASE...64b6de9a

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `docs/content/integrations/forges/webhooks.md:248` | Log filter still names `df_agent_channel::webhooks`; target is now `df_orchestrator::webhooks` | fix: update page, note docs change in proposal/tasks | fixed (7e96896e) |
| S2, S3 | Standards | P1 | `crates/df-agent-channel/src/testing.rs:194-245` | Channel test `Env` holds two copies of `OrchestratorContext`; `env.ctx.orch.config = …` updates only one; `other_replica` duplicates orchestrator fixture | fix: one copy | fixed (7e96896e) |
| P3 | Spec | P1 | `crates/df-agent-channel/src/error.rs:46-77` | Status pinning test does not cover a wrapped `OrchestratorError::Db` | fix: add case | fixed (7e96896e) |
| S4 | Standards | P1 | `crates/df-orchestrator/src/scheduler/tests.rs:345-365` | Test reimplements `pending_prompts` logic | fix: assert stored state | fixed (7e96896e) |
| S1, P2, P4, P5 | Standards/Spec | P1 | `crates/df-agent-channel/src/lib.rs:94`, `design.md` | `Deref` on `ChannelContext` deviates from decision 2 and hides `OrchestratorConfig` behind `ctx.config`; extra `Sqlx` variant; no staged untangle. Undocumented deviations | fix: record in design.md (keep Deref) | fixed (7e96896e) |
| S5 | Standards | P1 | `crates/df-agent-channel/src/error.rs:10-27` | Overlapping `Db`/`Sqlx`/`Json`/… variants in `ChannelError` and `OrchestratorError` | fix: unify (user moved from P3) | fixed (7e96896e) |
| S6 | Standards | P3 | `crates/df-agent-channel/src/session.rs:39`, `tools.rs:26` | `df_orchestrator` import grouping inconsistent | — | cosmetic |

Security: 0 findings.

User decision: Fix blocking + P1, plus S5 (unify the error variants). S6 stays cosmetic.

## Round 2 (2026-11-18)

- Implementer: 7e96896e (fix round 1), checks: fmt, clippy, machete, xtask checks, docs-build, nextest 438 pass (channel 231, orchestrator 138, dark-factory 69), Bazel tests 6/6; container tests ran; e2e not rerun
- Reviewed range: 64b6de9a...7e96896e
- Round 1 items verified fixed by reviewers.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-S1 | Standards | P1 | `crates/df-orchestrator/src/testing.rs:51,122` | `impl Fixture` not next to `struct Fixture` (AGENTS.md file structure) | fix: move impl | fixed (6b7c706e) |
| R2-S3 | Standards | P1 | `crates/df-agent-channel/src/error.rs:18-30` | `via_orchestrator!` macro hides four `From` impls; plain impls are greppable | fix: plain impls | fixed (6b7c706e) |
| R2-P1 | Spec | P1 | `openspec/changes/orchestrator-crate/design.md:73` | Risks bullet still describes staged untangle, contradicts decision 1 "as built" | fix: reword | fixed (6b7c706e) |
| R2-S2 | Standards | P3 | `crates/df-orchestrator/src/testing.rs:81-99` | Orchestrator `Env::with_events`/`other_replica` only forward to free functions | — | cosmetic |

Security: 0 findings.

User decision: Fix P1 (R2-S1, R2-S3, R2-P1).

## Round 3 (2026-11-18)

- Implementer: 6b7c706e (fix round 2), checks: fmt, clippy, machete, xtask checks, nextest 438 pass, Bazel tests 6/6, e2e-local pass; container tests ran
- Reviewed range: 7e96896e...6b7c706e, checked by the orchestrator inline (four plain `From` impls, one reworded design bullet, test-fixture reorder verified as move-only). No findings.

## Finish

- Bazel `build --lockfile_mode=error //crates/... //plugins/...`: ok
- `just docs-build`: ok. Published page changed: `docs/content/integrations/forges/webhooks.md` (log filter target)
- Tech debt: none (no P2 findings)
- Roadmap: none (out-of-scope items were rejected alternatives, not deferred work)
