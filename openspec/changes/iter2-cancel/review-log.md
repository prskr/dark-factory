# Review log: iter2-cancel

- BASE: 401c7f22ae80e4c0f2a8ae03d30c8171584a272b
- Store: none
- Status: done
- Implementer agent: a53151e2330c16261

## Design (2026-11-02)

- Commit: 70f1727 (`wip(iter2-cancel): design`), task 3.1 done, approved by the user.
- PenPot: `Status badge` variant `Cancelled`; boards Desktop · stop and cancel, Desktop · cancel task confirmation, Phone · stop and cancel, Phone · cancel task confirmation, Phone · stop from composer.
- Scope addition approved by the user: composer Send turns into Stop while the task is `running` and the message box is empty (design.md D3, task 3.2).

## Round 1 (2026-11-02)

- Implementer: d1c9d67, d39b5de; tasks 9/9; checks fmt/clippy/machete ok, cargo tests df-core, df-db, df-agent-channel, df-web, dark-factory pass; lefthook not on PATH (hooks not run); Bazel not run; e2e tests in df-e2e compiled, not run (need cluster)
- Container tests: ran (podman socket)
- Reviewed range: 401c7f2...d39b5de

Security: 0 findings. Pre-commit hooks (run by the orchestrator via `.bazel_env`): pass. `cargo deny`: fails on pre-existing advisory `pulldown-cmark 0.13.5` (lock file unchanged by this change).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-db/src/tasks.rs:885` | Stop on a `finalizing` run (composer Stop) sets the marker; messages queued before are ignored, task goes `in_review` with undelivered messages | fix-now | fixed (36c0a77) |
| P2 | Spec | Blocking | `crates/df-agent-channel/src/pr.rs:58-80` | PR marked ready + summary for a task cancelled while its run finalizes after `complete_task`; draft PR opened for a cancelled task | fix-now (+ skip `cancelled` in `try_ensure_draft`, spec extended) | fixed (36c0a77) |
| P3 | Spec | Blocking | `crates/df-agent-channel/src/scheduler.rs:118-168` | Stop during provisioning: teardown before `provider.provision` returns leaks the sandbox | fix-now | fixed (36c0a77) |
| P5 | Spec | Blocking | `crates/df-agent-channel/src/session/tests.rs` | "Stop a running run" has no fake-ACP end-to-end test (task 2.1) | fix-now | fixed (36c0a77) |
| S1, P8 | Standards, Spec | P1 | `crates/df-agent-channel/docs/configuration.md:34-72`; task 4.1 | Run lifecycle reference lacks human stop, stop timeout, `cancelled`, queued-before-stop rule; no how-to | fix-now | fixed (36c0a77) |
| P4 | Spec | P1 | `design.md` D1 | D1 still says the web handler tears down unconnected runs | fix-now | fixed (36c0a77) |
| P7 | Spec | P1 | `crates/df-web/src/task_pages/tests/stop_cancel.rs` | No SSE test for live re-render after stop | fix-now | fixed (36c0a77) |
| S2 | Standards | P1 | `crates/df-agent-channel/src/runs.rs` | Loop test asserts nothing for `Failed` | fix-now | fixed (36c0a77) |
| S3 | Standards | P1 | `crates/df-web/src/task_pages.rs:318-343` | Mark-ready duplicates `pr.rs:61-74` | fix-now | fixed (36c0a77) |
| S5 | Standards | P1 | `crates/df-agent-channel/src/pr.rs:118-130` | Status compared as string | fix-now | fixed (36c0a77) |
| S6 | Standards | P1 | `crates/df-web/src/sse.rs:107-113` | Stale region doc comments | fix-now | fixed (36c0a77) |
| P6 | Spec | P1 | `tasks.md` 2.2 | Task text names `tokio::time::pause`, test uses explicit `now` | fix-now (task text) | fixed (36c0a77) |
| S4 | Standards | P2 | `crates/df-db/src/tasks.rs` | Two more copies of the terminal run-state list | tech-debt (extend existing entry, tech_debt.md:635) | tech-debt (TD-65, extended) |

User decision: Fix blocking + P1. `try_ensure_draft` also skips `cancelled` tasks; spec delta `pull-request-lifecycle` extended (scenario "Cancelled before the first PR").

## Round 2 (2026-11-02)

- Implementer: 36c0a77, tasks 9/9, checks fmt/clippy/machete/typos ok (hooks ran), tests df-core, df-db, df-agent-channel, df-web pass; Bazel and `dark-factory` tests not re-run
- Container tests: ran (podman socket)
- Reviewed range: c34dd85...36c0a77

Security: 0 findings. Bazel build (`--lockfile_mode=error`): not run, environment fails with "No space left on device" under ~/.cache/bazel (df shows 164G free); user will fix, retry in finish step.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S7 | Standards | Blocking | `crates/df-db/src/tasks.rs:810,850,890` | `request_stop_tx` bool flag argument | fix-now (enum) | fixed (d99cd38) |
| S10 | Standards | Blocking | `crates/df-db/src/tasks/tests.rs:1102` | Test name covers half the assertions | fix-now (split) | fixed (d99cd38) |
| P9 | Spec | Blocking | `design.md` D5, `tasks.md` 2.4 | Name only `completed` for draft skip | fix-now (orchestrator) | fixed (1506903) |
| P10 | Spec | Blocking | `specs/run-orchestration/spec.md` Stopping a run | Finalizing-run exception missing from spec | fix-now (orchestrator: sentence + scenario "Stop while finalizing") | fixed (1506903) |
| P11 | Spec | Blocking | `tasks.md` 2.1 | Task text asks for fake-ACP test; coverage is split | fix-now (orchestrator: task text) | fixed (1506903) |
| P12 | Spec | Blocking | `crates/df-web/README.md:479`, `crates/df-agent-channel/docs/configuration.md:53-57` | How-to says every stop goes `finalizing`; diagram lacks preparing/awaiting_input → finalizing | fix-now | fixed (d99cd38) |
| S8 | Standards | Blocking | `crates/df-web/src/task_pages/view.rs:439` | `composer_action` param `running` means stoppable | fix-now (rename) | fixed (d99cd38) |
| S9 | Standards | Blocking | `crates/df-web/src/task_pages/view.rs:390-396` | Two positional bools in `message_area` | fix-now | fixed (d99cd38) |
| S11 | Standards | Blocking | `crates/df-agent-channel/src/runs.rs:718` | Hard-to-read test name | fix-now (rename) | fixed (d99cd38) |

All round 1 items confirmed fixed by both reviewers. Convergence rule: these are lower-severity but in code the fix round changed, hence blocking.
User decision: Fix all 9. Orchestrator edits P9–P11 (artifacts).

## Round 3 (2026-11-02)

- Implementer: d99cd38, tasks 9/9, checks fmt/clippy/machete/typos/test-names/file-length ok (hooks ran), tests df-db, df-agent-channel, df-web pass
- Container tests: ran (podman socket)
- Reviewed range: 1506903...d99cd38

Security: 0 findings. Spec: 0 findings; P9–P12 confirmed fixed. Standards: S7–S11 confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S12 | Standards | Blocking (convergence rule) | `crates/df-db/src/tasks.rs:894` | Doc line 118 chars (> 100, `.editorconfig`) | fix-now | fixed (5d3a282) |
| S13 | Standards | Blocking (convergence rule) | `crates/df-web/src/task_pages/view.rs:437` | Doc line 111 chars | fix-now | fixed (5d3a282) |
| S14 | Standards | Blocking (convergence rule) | `crates/df-web/src/task_pages/view.rs:436` | Tautological doc wording | fix-now | fixed (5d3a282) |

User decision: fix in a last mini round; orchestrator checks the diff instead of a full review round.

- Fix round 3: 5d3a282 (comments only, verified by the orchestrator: no non-comment line changed).

## Finish (2026-11-02)

- Container tests: ran in every round (podman socket). df-e2e flow tests (stop/cancel steps) compiled, not run (need the cluster).
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` passed after the environment's disk issue was fixed.
- Hooks: pre-commit ran on all implementer commits from round 2 on; round 1 commits checked by the orchestrator. `cargo deny` fails on pre-existing advisory `pulldown-cmark 0.13.5` (not caused by this change).
- Tech debt: TD-65 extended (S4); no new TD IDs.
- Roadmap: backlog row "Interrupt the current turn without ending the run".
- Status: done. No blocking finding open.
