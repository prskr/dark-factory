# Tasks

## 1. Provider and storage

- [ ] 1.1 Add `delete_task_volume` to `SandboxProvider`, implement it in `df-k8s` and the fake provider. Verify "volume deleted" and "already gone" against the Kubernetes API mock used by the provider tests
- [ ] 1.2 Add the migration (`tasks.volume_deleted_at`) and regenerate `.sqlx`; clear the marker in `ensure_task_volume`. Verify "recreated on the next run" (container-backed; CI)

## 2. Retention

- [ ] 2.1 Implement the hourly retention scan on the leader with `volumes.retainAfterClose` and `volumes.idleTtl` (design D1). Verify "closed task after the grace period", "idle task", "active run is skipped", "scan runs once per hour" and "run start during deletion" (a scheduler tick while the scan holds the row lock creates no run until the deletion committed) with `df_test_postgres`, a fake provider and `tokio::time::pause()` (container-backed; CI)

## 3. Reset action

- [ ] 3.1 PenPot: add the "Reset workspace" action to the task detail boards (Desktop and Phone pages) and a board for its confirmation, which names what is lost (design D2), and the rejection while a run is active. Export the boards, get the user's approval, and add the mapping rows to `docs/design-workflow.md` (design D4)
- [ ] 3.2 Implement the reset handler with the active-run check and audit entry (design D2) and the task detail action with confirmation. Verify "reset a waiting task", "reset rejected while running" and the rendering of the confirmation. Compare with the approved boards (`docs/design-workflow.md`, "Comparing a view") at 390 and 1440 px

## 4. Docs

- [ ] 4.1 Update `design/agent-run.md` §5.5 (status: implemented, settings) and the operator configuration reference; add "reset workspace" to the task page user guide
