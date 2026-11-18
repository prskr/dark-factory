# Review log: iter3-task-tree

- BASE: 122d74bdba279dfe8f84a764d43091050fac5657
- Store: none
- Status: done
- Implementer agent: a6290f987c79d61bf

## Design (2026-11-18)

- PenPot page `Tasks - Roadmap` renamed to `Tasks - Subtasks` (user request).
- New boards: `Tasks - Subtasks / Desktop · subtasks tab` and `Tasks - Details Sidebar / Desktop · overview, base task`; mapping rows in `docs/design-workflow.md`.
- Approved by user, with the page rename.
- Design commit: 9fe0312a

## Round 1 (2026-11-18)

- Implementer: 00d87284, tasks 20/22 (open: 7.1 e2e written, not run; 7.2 box open), checks fmt/clippy/machete/file-length/docs-build/helm-unittest ok; cargo tests 2653 passed (socket); factory-agent `--test sandbox` 14 failures also on BASE (environmental), container tests ran except df-e2e cluster tests (`just e2e-local`) and `bazel test`
- Reviewed range: BASE...00d87284
- Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-orchestrator/src/base_branch.rs:20` | base branch created only on run 1; resumed task after forge error never recovers | fix-now | open |
| P2 | Spec | Blocking | `crates/df-db/src/tasks/tree.rs` open_subtasks, `merge.rs` | parent can complete before a based subtask's PR is opened (pending `pr.sync`); e2e `sleep 8`; how-to sentence wrong | fix-now | open |
| S1, S2 | Standards | Blocking | `tree.rs` (8), `git_pushes.rs:250`, `task_events.rs:159` | new queries not checked macros | fix-now | open |
| S7 | Standards | P1 (downgraded: gateway reads `GitRoute.base_branch` only in tests) | `crates/df-db/src/egress.rs:224` | effective-base CTE duplicated | fix-now | open |
| P3 | Spec | P1 | `merge.rs:358-432` | checks without row lock, no re-check in record_merge | fix-now | open |
| P5 | Spec | P1 | — | no test that human mark-done isn't gated | fix-now | open |
| S3 | Standards | P1 | `tree.rs:154` | status list spelled out | fix-now | open |
| S5 | Standards | P1 | `df-e2e/tests/task_tree.rs:120,131` | copied e2e helpers | fix-now | open |
| S8 | Standards | P1 | `task_views.rs:324,467`, `subtasks.rs:48` | done/total rule thrice | fix-now | open |
| S9 | Standards | P1 | `tree.rs:16` | DelegationLimits mirrors TasksConfig | fix-now | open |
| S10 | Standards | P1 | `tree.rs:209` | bad pr_number becomes "no PR" | fix-now | open |
| P4, P7, P8 | Spec | P1 | design.md D7/D9/D10, task 6.1, spec | artifacts describe unbuilt details / miss built rules | fix-now | open |
| P9 | Spec | P1 | `subtasks.rs:15` | all-cancelled → no chip; reword spec | fix-now | open |
| P6 | Spec | P1 | `df-e2e/tests/task_tree.rs` | e2e not run | finish step 1 | open |
| S4, S6 | Standards | P3 | `subtasks.rs`, `delegation.rs:16` | item order | — | cosmetic |
| S11 | Standards | P3 | plugin merge.rs | similar precondition in two plugins | — | cosmetic |
| S12 | Standards | P3 | `task_tree.rs:104` | format! vs task_branch() | — | cosmetic |
| S13 | Standards | P3 | `df-orchestrator/src/base_branch.rs` | module/test placement | — | cosmetic |

User decision: Fix blocking + P1.

## Round 2 (2026-11-18)

- Implementer: b101f0bd (fix round 1), items 1-13 reported fixed; fmt/clippy/hooks ok, `.sqlx/` regenerated; 2028 tests passed (socket) in df-core, df-db, df-orchestrator, df-agent-channel, df-web, fake-acp-agent, df-e2e; machete not re-run; df-e2e cluster tests and bazel not run
- Reviewed range: 00d87284...b101f0bd
- Round 1 rows P1, P2, S1, S2, S3, S5, S7 (pin test), S8, S9, S10, P3 (defects → P10, P11), P4, P5, P7, P8, P9: fixed (b101f0bd). P6: open (finish step). Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P10 | Spec | Blocking | `crates/df-agent-channel/src/tools/delegation/merge.rs:126` | record_merge locks task row before run rows (deadlock with run end/cancel) | fix-now | open |
| S14 | Standards | Blocking | `crates/fake-acp-agent/src/agent.rs:562-583` | mcp retry sleep ignores cancellation | fix-now | open |
| S15 | Standards | Blocking | `crates/fake-acp-agent` retry | no tests for retry option | fix-now | open |
| P11 | Spec | Blocking | `merge.rs:100`, design D7 | message promises webhook fallback that never applies | fix-now | open |
| P12 | Spec | Blocking | `specs/task-tree/spec.md` Merge outcome | merged-not-marked-done path not in spec | fix-now | open |
| P13 | Spec | Blocking | `crates/df-agent-channel/docs/configuration.md:38` | open-subtask rule misses pending-PR window | fix-now | open |
| S18 | Standards | Blocking | `crates/df-db/src/tasks/tree.rs:72-84`, tests | PrSyncJob JSON shape restated in SQL and tests | fix-now | open |
| S16 | Standards | Blocking | `merge.rs:96` | hand-written CLOSED set | fix-now | open |
| S17 | Standards | Blocking | `merge.rs:100` | needless rebinding | fix-now | open |
| S19 | Standards | Blocking | `docs/architecture.md:51` | "first start" stale | fix-now | open |
| P14 | Spec | Blocking | `design.md:113-122` | D7 list numbering, stale transaction sentence | fix-now | open |
| P15 | Spec | — | `crates/df-db/src/tasks/tree.rs:561` | negative pr_number → InvalidData | — | rejected (intended, S10 applied consistently) |

User decision: Fix blocking + P1.

## Round 3 (2026-11-18)

- Implementer: d80709cf (fix round 2), items 1-11 reported fixed; fmt/clippy/machete/docs-build/openspec validate ok; 742 tests passed (socket) in df-db, df-agent-channel, fake-acp-agent, df-e2e; `crates/fake-acp-agent/Cargo.toml` gained a tokio `test-util` dev-dependency (bazel build needed at finish); df-e2e cluster tests and bazel not run
- Reviewed range: b101f0bd...d80709cf
- Round 2 rows P10, P11, P12, P13, P14, S14, S15, S16, S17, S18, S19: fixed (d80709cf). Security: 0 findings.
- Bazel build `--lockfile_mode=error //crates/... //plugins/...` at d80709cf: ok.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P17, S20 | Spec, Standards | Blocking | `docs/content/using/delegation.md:99-105` | merged-but-done/cancelled outcome not in published docs | fix-now | open |
| P16 | Spec | Blocking | `crates/df-agent-channel/docs/configuration.md:38` | says `failed` never counts as open; code counts a failed based subtask with PR/pending job | fix-now | open |
| S22 | Standards | Blocking (convergence) | `crates/df-agent-channel/src/tools/delegation/merge_tests.rs:193-233` | deadlock test probabilistic | fix-now | open |
| S21 | Standards | Blocking (convergence) | `merge_tests.rs:187-190` | "is done" assertion in a test named for another case | fix-now | open |
| S23 | Standards | Blocking (convergence) | `crates/fake-acp-agent/src/tests.rs:536` | spawned task handle dropped | fix-now | open |

User decision: Fix blocking + P1.

## Round 4 (2026-11-18)

- Implementer: ebe23ab6 (fix round 3), 5 items reported fixed; fmt/clippy/machete/docs-build ok; df-agent-channel 228, df-db 397, fake-acp-agent pass (socket); deadlock test verified to fail (40P01) with the old lock order
- Reviewed range: d80709cf...ebe23ab6
- Round 3 rows P16, P17/S20, S21, S22, S23: fixed (ebe23ab6). Spec: 0 new. Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S24 | Standards | Blocking (convergence) | `docs/content/using/delegation.md:107-110` | "Merges into the default branch stay with humans." glued to the edge-case paragraph | fix-now | open |
| S25 | Standards | Blocking (convergence) | `docs/content/using/delegation.md:109-110` | cancelled case stated twice | fix-now | open |

User decision: fix-round limit reached; one more doc-only round (S24, S25), orchestrator checks the lines, then finish. Combined with finish step 1 (container/e2e tests).

- S24, S25: fixed (28a9ad00), checked by the orchestrator.

## Round 5 (2026-11-18): e2e run

- `just e2e-local` at 28a9ad00: 1 failure, `task_tree::a_feature_agent_merges_its_iterations_and_the_next_one_starts_on_the_merge` (TT-3 never completed). Causes: (1) gateway re-checked the base task's branch commits, rejecting the forge's merge commit (`df:identity`, author factory@example.com) on all 6 pushes of iteration two; (2) test waited for `in_review` although the merge's done message wakes the feature task again; (3) test read the hidden Subtasks panel.
- Fix 82da08a8: run's base branch counts as upstream in push checks (`df-gateway` `TaskRefs::counts_as_upstream`), test `commits_of_the_base_branch_are_upstream` (fails without the fix), README sentence; e2e wait and Subtasks tab fixed. Orchestrator accepted the broader variant over "exempt forge committer only" (base branch = the subtask PR's own target); Security axis reviews it.
- Full `just e2e-local` on the fixed tree: 37 passed, 0 failed. Bazel test and remaining cargo crates: running.
- Reviewed range: 28a9ad00...82da08a8
- Note: correction to round 1 — `GitRoute.base_branch` is now read by the push checks.
- Security: 0 findings (base branch is always a real task branch or the project base; subtask PR targets the same branch; ref form matches). One human-gated gap below threshold: a base task marked Done unmerged, then the subtask's PR merged.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P18 | Spec | Blocking | `specs/` (no push-checks delta), `crates/df-gateway/src/git/view.rs:66` | base branch upstream contradicts main push-checks "Checked commits" | fix-now (spec delta + design decision) | open |
| P19 | Spec | Blocking | design.md Risks, `crates/df-gateway/README.md:250` | accepted risk not recorded | fix-now (docs only) | open |
| P20 | Spec | Blocking | `crates/df-gateway/src/git/policy_checks.rs:111` | no push-level test of the base-branch wiring | fix-now | open |
| P21 | Spec | Blocking | `crates/df-e2e/tests/task_tree.rs:119-125` | feature PR looked up once, async job may race | fix-now | open |
| S26 | Standards | P3 | `crates/df-e2e/src/browser.rs:593` | tab locator by id | — | cosmetic |
| S27 | Standards | P3 | `crates/df-e2e/tests/task_tree.rs:103` | inline wait repeats part of wait_for_runs | — | cosmetic |

User decision: accept the broad variant (run's base branch counts as upstream); fix P18–P21.

## Round 6 (2026-11-18)

- Implementer: 6a766a34 (fix round 5; ticks 7.1/7.2). P18 delta `specs/push-checks/spec.md` (MODIFIED "Checked commits", existing scenarios kept, `openspec validate --strict` ok), design D2 decision, Risks entry, README; two push-level tests (fail without the wiring); e2e PR wait.
- Suites: full `just e2e-local` 37/37 at 82da08a8 tree (before round 5 edits); both task_tree tests pass after round 5 on a fresh cluster. Bazel `//crates/... //plugins/...` 48/49 targets, `//crates/factory-agent:sandbox_test` 10 failures (environmental Landlock/sandbox, also on BASE). Cargo `--all-features` at 82da08a8: df-core, df-orchestrator, df-web, df-gateway-control, df-gateway, df-plugins, forge-common, forgejo/github plugins pass; df-gateway at 6a766a34 passes. fmt/clippy/machete/check-file-length/openspec validate ok.
- Reviewed range: 82da08a8...6a766a34. Security skipped (tests/docs/spec only, excluded categories).
- Round 5 rows P18, P19, P20, P21: fixed (6a766a34).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S28 | Standards | Blocking | `crates/df-e2e/tests/task_tree.rs:121` | PR wait on pr.sync uses UI_TIMEOUT (flow.rs uses RUN_TIMEOUT) | fix-now | open |
| S29 | Standards | Blocking (convergence) | `crates/df-gateway/tests/push_checks.rs:441` | commit_as_forge copies commit fixture | fix-now | open |
| S30 | Standards | Blocking (convergence) | `push_checks.rs:441` | fixture placed between tests | fix-now | open |
| S31 | Standards | Blocking (convergence) | `push_checks.rs:424,483` | magic `factory/task/DF-8` | fix-now | open |

User decision: fix S28–S31, orchestrator verifies the diff, then finish.

- S28, S29, S30, S31: fixed (36eb8dfa), diff checked by the orchestrator; df-gateway all binaries pass, clippy/fmt ok, df-e2e tests compile.

## Finish (2026-11-18)

- Container tests: ran (DOCKER_HOST=unix:///var/run/docker.sock), incl. full `just e2e-local` (37/37) and both task_tree tests after the last behavior change. `factory-agent --test sandbox` / `//crates/factory-agent:sandbox_test` failures are environmental (Landlock/sandbox) and fail on BASE too.
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` ok at d80709cf; `test //crates/... //plugins/...` 48/49 targets (sandbox_test environmental).
- Docs: `just docs-build` ok at 36eb8dfa. Pages added/changed: docs/content/using/{build-a-feature.md (new), delegation.md, tasks.md, setup.md, artifacts.md, _index.md}, docs/content/architecture/agent-runs.md, docs/content/integrations/forges/_index.md, crates/{df-agent-channel,df-plugins,factory-agent}/docs/configuration.md, crates/df-core/docs/task-transitions.md.
- Tech debt: none (no P2 findings). Roadmap: no new rows; out-of-scope items already in the backlog (base-branch notices, re-parenting, `base_task` in the composer, `subtask_merge` override, project limit overrides, read-only reviewer mode).
- Cosmetic: S4, S6, S11, S12, S13, S26, S27. Rejected: P15.
- Below-threshold security note (not a finding): a base task marked Done without merging, followed by a human merging the subtask's PR, lets the subtask's earlier commits skip re-checks.
