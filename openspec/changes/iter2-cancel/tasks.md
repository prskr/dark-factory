# Tasks

## 1. Storage

- [ ] 1.1 Add the migration (task status `cancelled`, `runs.stop_requested_at`, prompt state `discarded`) and regenerate `.sqlx`. Verify "status is always one of the defined values" with `cancelled` accepted (container-backed; CI)

## 2. Control plane

- [ ] 2.1 Implement stopping a run (design D1) for connected and unconnected runs. Verify "stop a running run" with the fake ACP agent and "stop while provisioning" with a fake `SandboxProvider`
- [ ] 2.2 Add the stop timeout to the timeout scan and the run-end mapping exception. Verify "runtime hangs" (`tokio::time::pause`), "stopped with an undelivered message" and "completed with an undelivered message"
- [ ] 2.3 Implement cancelling and reopening a task (design D2). Verify "cancel a running task", "cancel a completed task", "reopen", "reopen before the stopped run ended" (also as "reopened while the stopped run finalizes" in `specs/run-orchestration/spec.md`) and "cancelled with a pending approval" (container-backed; CI)
- [ ] 2.4 Mark the PR ready when a task is marked done (design D5). Skip draft creation for `completed` tasks in `try_ensure_draft`. Verify "close a waiting task with a PR", "mark done without a PR" and "marked done before the first PR" with a fake forge, and that a forge error leaves the task `completed`

## 3. UI

- [ ] 3.1 PenPot: add "Stop" for an active run and "Cancel task" to the task detail boards (Desktop and Phone pages), plus a board for the cancel confirmation dialog and the `cancelled` status badge (Components page, `Status badge`). Export the boards, get the user's approval, and add the mapping rows to `docs/design-workflow.md` (design D4)
- [ ] 3.2 Add the stop and cancel actions with confirmation and live updates (design D3) with the layout of the approved boards. Verify "stop on a phone" and "cancel needs confirmation" with handler and rendering tests, and once by hand on a phone. Compare with the approved boards (`docs/design-workflow.md`, "Comparing a view") at 390 and 1440 px

## 4. Integration

- [ ] 4.1 Extend the e2e test with demo step 5 of `product/iterations/iteration-2.md` (stop → `waiting`; cancel → `cancelled`, requests closed, PR untouched). Document stop and cancel in the user guide for the task pages
