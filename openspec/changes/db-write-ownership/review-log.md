# Review log: db-write-ownership

- BASE: d9cc3cf9ec7ed6c82de5927796c1a7b1660d7daa
- Store: none
- Status: done
- Implementer agent: a7d4dd62f45905fb9

## Round 1 (2026-11-17)

- Implementer: eca341ab + a975cd75 (HEAD_1), tasks 31/31, checks fmt/clippy/machete/xtask/docs-build/bazel build+test ok, e2e ok, container tests ran (docker.sock)
- Reviewed range: BASE...a975cd75

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1, P1 | Standards+Spec | P1 | `docs/architecture.md:47,70` | df-db row still `target: db-write-ownership`, rule 2 "once … lands" | fix-now | fixed (e1be0ba83fc539adb834880f87c457e080047941) |
| S2 | Standards | P1 | `crates/df-agent-channel/src/testing.rs:4` | module-level exemption on `test-util` module not in documented exemption list | fix-now | fixed (e1be0ba83fc539adb834880f87c457e080047941) |
| P2 | Spec | P1 | `tasks.md` 7.2 | guard probe result not recorded | fix-now | fixed (e1be0ba83fc539adb834880f87c457e080047941) |
| P3 | Spec | P1 | `.agents/skills/rust/SKILL.md:126` | skill omits `execute_many` | fix-now | fixed (e1be0ba83fc539adb834880f87c457e080047941) |
| S3, P4 | Standards+Spec | P1 | `crates/df-agent-channel/src/runs.rs:6,13` | re-export + `get_run` wrapper (Middle Man) | fix-now | fixed (e1be0ba83fc539adb834880f87c457e080047941) |
| S8 | Standards | P1 | `crates/df-db/src/transcript.rs:125` | `append_runtime` restates kind rule instead of `allowed_from_runtime` | fix-now | fixed (e1be0ba83fc539adb834880f87c457e080047941) |
| S10 | Standards | P1 | `docs/tech_debt.md:79,397` | TD-66 location stale | fix-now | fixed (e1be0ba83fc539adb834880f87c457e080047941) |
| S4 | Standards | P3 | `crates/df-agent-channel/src/pr.rs:22` | `lock_task_pr` re-export | fix-now (user) | fixed (e1be0ba83fc539adb834880f87c457e080047941) |
| S9 | Standards | P3 | `crates/df-db/src/tasks.rs:1194` | literal state list in `request_stop_tx` CASE | fix-now (user) | fixed (e1be0ba83fc539adb834880f87c457e080047941) |
| P5 | Spec | P3 | `design.md` D9 | NULL bound for intervals > ~100 years, wording | fix-now (user) | fixed (e1be0ba83fc539adb834880f87c457e080047941) |
| S6 | Standards | P3 | channel, 6 sites | repeated `get_task(..).ok_or_else` | cosmetic | cosmetic |
| S7 | Standards | P3 | `crates/df-agent-channel/src/session.rs:505` | active-run rule repeated | cosmetic | cosmetic |
| S5 | Standards | Rejected | 4 `TranscriptWriter` call sites | one-shot `append_control_event` helper | — | rejected (design decision 4 replaces it deliberately) |

Security: 0 findings.

User decision: Fix blocking + P1 + S4 + S9 + P5

## Round 2 (2026-11-17)

- Implementer: e1be0ba83fc539adb834880f87c457e080047941 (HEAD_2), fixed S1/P1, S2, P2, P3, S3/P4, S4, S8, S9, P5, S10; checks clippy/fmt/machete/xtask/openspec ok; container tests ran (docker.sock); .sqlx unchanged
- Reviewed range: a975cd75...e1be0ba83fc539adb834880f87c457e080047941
- Round 1 rows S1/P1, S2, P2, P3, S3/P4, S4, S8, S9, P5, S10: fixed (e1be0ba83fc539adb834880f87c457e080047941), confirmed by Standards and Spec.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S11 | Standards | P3 | `crates/df-agent-channel/src/assign.rs:31` + 7 files | `df_db` imports placed in the `crate::` group | fix-now (user) | fixed (7e5f26e3) |
| S12 | Standards | P3 | `crates/df-db/src/transcript.rs:126` | error text names the two kinds instead of the rejected kind | fix-now (user) | fixed (7e5f26e3) |

Security: 0 findings. Spec: 0 findings.

User decision: Fix S11 + S12

## Round 3 (2026-11-17)

- Implementer: 7e5f26e3 (HEAD_3), fixed S11, S12; clippy/fmt ok; df-db and df-agent-channel tests ran with docker.sock; .sqlx unchanged
- Checked by orchestrator (cosmetic-only diff, 9 files): S11 fixed (7e5f26e3), S12 fixed (7e5f26e3)

## Finish (2026-11-17)

- Container-backed tests: ran in every round (docker.sock); e2e (`just e2e-local`) ran in round 1, all passed
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` ran in round 1 after the Cargo/MODULE.bazel.lock change; fix rounds touched no Cargo files
- Docs: `just docs-build` ok; no published docs page changed (pure refactor); contributor docs changed: AGENTS.md, `.agents/skills/rust/SKILL.md`, `docs/architecture.md`, `docs/tech_debt.md`, ADR-0057
- Tech debt: none (no P2 findings); TD-14, TD-19, TD-23, TD-33, TD-42, TD-45, TD-65 removed by the change
- Roadmap: backlog row "Convert the remaining runtime-checked queries of `df-db`"
- Status: done
