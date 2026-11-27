# Review log: task-list-tree

- BASE: f1e6dce9
- Store: none
- Status: done

## Round 1 (2026-11-27)

- Implementer: 25928750, 8ec00925 (HEAD_1), tasks 15/15, checks fmt/clippy/machete/openspec validate/docs-build ok, nextest df-core/df-db/df-web 2056 passed, container tests ran (DOCKER_HOST=unix:///var/run/docker.sock), `just e2e-local` green
- Reviewed range: f1e6dce9...8ec00925
- Security: 0 findings

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-web/src/task_pages/tree.rs:84,128,134` | Task with only cancelled subtasks gets no disclosure control, can't be opened (spec: "with subtasks") | fix-now | fixed (7a69fae0) |
| P2 | Spec | Blocking | `crates/df-web/src/sse/tests.rs` | "Two levels down": middle row's dot not tested live | fix-now | fixed (7a69fae0) |
| P3 | Spec | Blocking | `crates/df-web/src/task_pages/view/tests.rs` | "Running parent without waiting subtasks" untested | fix-now | fixed (7a69fae0) |
| P4 | Spec | Blocking | `crates/df-web/src/task_pages/tests/tree.rs` | Breadcrumb test doesn't check `project` is kept | fix-now | fixed (7a69fae0) |
| P5 | Spec | P1 | `crates/df-db/src/task_views.rs` needs-you SQL | Chat parent = no parent task for needs-you: untested | fix-now | fixed (7a69fae0) |
| S1 | Standards | P1 | `crates/df-web/src/task_pages/tree.rs:104-279` | Private items before public `task_rows`/`breadcrumb` | fix-now | fixed (7a69fae0) |
| S2 | Standards | P1 | `ListScope::ChildrenOf`, `TaskList::children`, test helper `child`, `a_grandchild_…` | Glossary _Avoid_ "child task" | fix-now | fixed (7a69fae0), remainder R2-S4 |
| S3 | Standards | P1 | `crates/df-db/src/task_views/tree_tests.rs` | Glossary _Avoid_ "root task" | fix-now | fixed (7a69fae0) |
| S4 | Standards | P1 (user) | `crates/df-db/src/task_views.rs:888` | Test modules ~760 lines, inline + sibling | fix-now | fixed (7a69fae0) |
| S5 | Standards | P1 | `crates/df-db/src/task_views.rs:~433` | PR-or-job SQL third copy; outbox query not pinned | fix-now | fixed (7a69fae0) |
| S6 | Standards | P1 (user) | `ListScope::Task(String)`, `Filters::open/parent` | Keys as String instead of TaskKey | fix-now | fixed (7a69fae0), remainder R2-S1 |
| S7 | Standards | P1 (user) | `crates/df-db/src/task_views.rs:145-157` | Repeated match on scope | fix-now | fixed (7a69fae0) |
| S8 | Standards | P1 | sse/task_pages/df-db tests | Subtask setup copied 3x, sse uses raw SQL | fix-now | fixed (7a69fae0) |
| S9 | Standards | P1 (user) | test helpers `row`/`row_of` | Row helper copied 4x | fix-now | fixed (7a69fae0) |
| S10 | Standards | P1 (user) | `crates/df-web/src/task_pages/tree.rs:222` | Param `ctl` | fix-now | fixed (7a69fae0) |
| S11 | Standards | P1 (user) | `crates/df-web/src/task_pages/tree.rs:28-48` | `scoped` accessor inconsistent with other fields | fix-now | fixed (7a69fae0) |

User decision: Blocking + P1 + S4 + S6 + S7 + S9 + S10 + S11 (every finding fixed in round 1)

## Round 2 (2026-11-27)

- Implementer: 7a69fae0 (HEAD_2), tasks 15/15, fixed P1-P5, S1-S11; checks fmt/clippy/machete/openspec validate/docs-build ok, nextest df-core/df-db/df-web 2061 passed, container tests ran; e2e not re-run (no e2e-relevant change)
- Reviewed range: 8ec00925...7a69fae0
- Security: 0 findings

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-P1 | Spec | P1 | `crates/df-web/src/task_pages/tests/tree.rs:196` | Breadcrumb test doesn't check `sort` is kept (code keeps it) | | open |
| R2-S1 | Standards | P1 | `crates/df-web/src/task_pages/tree.rs:178,185`, `load_list` | `TaskListItem.key`, `Ancestor.key`, `TaskList.subtasks` keys still `String`; parse back and forth, link silently dropped on parse failure | | open |
| R2-S2 | Standards | P1 | `crates/df-web/src/testkit.rs:786` | `new_subtask` repeats `new_task`'s `NewTask` literal | | open |
| R2-S3 | Standards | P1 | `crates/df-db/src/task_views.rs:146` | SQL alias `listed` doesn't say it counts all subtasks | | open |
| R2-S4 | Standards | P1 | `crates/df-web/src/task_pages/tests/tree.rs:115` | Assertion message "grandchildren" (glossary _Avoid_) | | open |

User decision: Fix all 5 (fix-now for R2-P1, R2-S1..R2-S4)

## Round 3 (2026-11-27)

- Implementer: 680aa5ed (HEAD_3), tasks 15/15, fixed R2-P1, R2-S1..R2-S4; checks fmt/clippy/machete/openspec validate/docs-build ok, nextest df-core/df-db/df-web 2061 passed, container tests ran; e2e not re-run
- Reviewed range: 7a69fae0...680aa5ed
- Security: 0 findings. Spec: 0 findings. Round 2 rows R2-P1, R2-S1..R2-S4: fixed (680aa5ed).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R3-S1 | Standards | P2 | `crates/df-db/src/task_views.rs:522` (also `tasks.rs:565,824`, `tasks/tree.rs:96`) | Key parse `map_err(KeyError → DbError::InvalidData)` written 4 times; 3 copies predate the change | fix-now (user) | open |

User decision: fix R3-S1 now; free Docker disk (rm test Postgres, system prune) and re-run e2e after the fix.

## Round 4 (2026-11-27)

- Implementer: 301f79fb (HEAD_4), fixed R3-S1; checks fmt/clippy/machete ok, nextest df-db/df-web 1706 passed, container tests ran
- Reviewed range: 680aa5ed...301f79fb (40 lines, mechanical refactor: `From<KeyError> for DbError` in `pool.rs`, 4 call sites use `?`, same variant and message, unit test added): checked by the orchestrator, no reviewers spawned. Security: not run.
- R3-S1: fixed (301f79fb). No new findings.

## Finish (2026-11-27)

- Container tests: ran every round (DOCKER_HOST=unix:///var/run/docker.sock). `just e2e-local` at 301f79fb: green (98 tests, 0 failed), after freeing Docker disk (user-approved).
- Bazel build: not needed (no `Cargo.toml`/`Cargo.lock` change).
- Docs: `just docs-build` ok. Pages changed: `docs/content/using/tasks.md`, `delegation.md`, `build-a-feature.md`.
- Tech debt: none (every finding fixed). Deferred work: 3 backlog rows in `product/iterations/roadmap.md` (proposal Out of Scope).
- Open proposal (not done): glossary terms "needs-you dot" and "scoped list".
