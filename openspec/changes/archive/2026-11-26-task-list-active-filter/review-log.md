# Review log: task-list-active-filter

- BASE: 089bba8f1532127b7d8feafedec1d2a606eb8811
- Store: none
- Status: done

Preflight: the specs changed after the design commit (54e87dff, the Done group wording adds `cancelled`). The user confirmed the UI scope did not change and started the implementation.

## Round 1 (2026-11-26)

- Implementer: 3c23ec06, tasks 4/4, checks fmt/clippy/machete ok, df-core 335, df-db 489, df-web 1111 pass, docs-build ok; container tests ran (docker.sock); e2e ran to the end without a failure artifact dir, per-binary counts not read (unverified)
- Reviewed range: 089bba8f...3c23ec06
- Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | P1 | `crates/df-core/src/domain.rs:23` | `StatusGroup` doc still says "used by the task list filters"; it now backs badges and counters only, and its `Active` differs from `StatusSet::Active` | fix-now (doc only, no rename) | fixed (e0173053) |
| SP1 | Spec | P1 | `crates/df-web/src/task_pages/tests/list.rs:678` | running→waiting live-update test asserts a loose `"waiting"` substring | fix-now | fixed (e0173053) |
| S2 | Standards | P1 | `crates/df-web/src/task_pages/tests/list.rs:697`, `~425` | "no chip is current" check duplicated in two tests | fix-now | fixed (e0173053) |
| S3 | Standards | P3 | `crates/df-core/src/domain.rs:784` | redundant `contains(&Waiting)` after the equality assert | fix-now (user) | fixed (e0173053) |

User decision: fix blocking + P1 + S3.

## Round 2 (2026-11-26)

- Implementer: e0173053, tasks 4/4, checks fmt/clippy/machete ok, df-core domain 25 pass, df-web task_pages 169 pass; container tests ran (docker.sock); `just e2e-local` 17 binaries, 0 failed (log checked by orchestrator)
- Reviewed range: 3c23ec06...e0173053
- Security: 0 findings. Round 1 rows S1, SP1, S2, S3: fixed (e0173053), confirmed by standards and spec.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP2 | Spec | Blocking (convergence) | `crates/df-core/src/domain.rs:22` | S1 fix says `StatusGroup` is behind the status badges; badges use `Tone::of_status` (orchestrator's unverified wording in the round 1 fix list) | fix-now | fixed (a6b3b661) |
| S4 | Standards | Blocking (convergence) | `crates/df-web/src/task_pages/tests/list.rs:679` | SP1 fix adds a 4th hand-rolled "cut out row DF-1" (others at 444, 558, 582) | fix-now | fixed (a6b3b661) |

User decision: fix blocking + P1.

## Round 3 (2026-11-26)

- Implementer: a6b3b661, tasks 4/4, checks fmt/clippy/machete ok, df-core domain 25 pass, df-web task_pages 169 pass; container tests ran (docker.sock); no e2e rerun (doc and test-only changes)
- Reviewed range: e0173053...a6b3b661
- Security: 0 findings. Spec: 0 findings. Round 2 rows SP2, S4: fixed (a6b3b661), confirmed by standards and spec.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S5 | Standards | P3 (user adjusted from Blocking) | `crates/df-web/src/task_pages/tests/list.rs:686` | new `row(html, key)` test helper duplicates `row` in `chat_pages/tests/mod.rs:82`; could move to `testkit.rs` | cosmetic | cosmetic |

User decision: Adjust, S5 → P3.

## Finish (2026-11-26)

- Container tests: ran (docker.sock) in every round; `just e2e-local` 17 binaries, 0 failed on e0173053 (later rounds doc and test-only).
- Bazel build: skipped, no `Cargo.toml` or `Cargo.lock` changed.
- Docs: `just docs-build` ok; changed page `docs/content/using/tasks.md`.
- Tech debt: none (no P2). Roadmap: none (out-of-scope items were rejected, not deferred).
- Fixed: S1, SP1, S2, S3, SP2, S4. Cosmetic: S5. Rejected: none.
