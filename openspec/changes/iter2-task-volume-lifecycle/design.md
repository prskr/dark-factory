# Design

## Context

Task volumes are RWO PVCs created by `ensure_task_volume` before a task's first run and kept after every run (`sandbox-provisioning`). Git holds committed work and the WIP snapshot, so a volume is a cache, not the source of truth (ADR-0009).

## Goals / Non-Goals

**Goals:**
- Storage of finished and abandoned tasks is reclaimed without an operator.
- A human can throw away a broken workspace from the task page.

**Non-Goals:**
- Deleting volumes on PR merge events (needs webhooks, iteration 3; "completed + grace" covers the usual case).
- Volume snapshots, clones from a per-project seed PVC, or shrinking.

## Decisions

### D1 — One scan, one marker
The scheduler leader runs the retention scan once per hour. It selects tasks without a non-terminal run and with `volume_deleted_at` null whose status is `completed` or `cancelled` with the last run older than `volumes.retainAfterClose`, or whose last run is older than `volumes.idleTtl`, and handles each candidate in its own transaction: lock the task row (`SELECT … FOR UPDATE`, the row the scheduler locks with `FOR UPDATE SKIP LOCKED` when it starts a run), re-check that no non-terminal run exists, call `delete_task_volume` with a 30 s timeout while holding the lock, set `tasks.volume_deleted_at`, commit. A run start for that task is skipped or waits until the deletion is committed, so no run can be admitted between the check and the deletion. `ensure_task_volume` clears the marker when it creates the PVC again. Deleting a PVC that doesn't exist succeeds. *ponytail:* a marker column instead of a volume table; the PVC is the state.

### D2 — Reset action
`POST /tasks/{key}/reset-workspace` locks the task row like D1, rejects tasks with a non-terminal run (409), otherwise calls `delete_task_volume` while holding the lock, sets the marker and writes the audit entry, and commits. The lock, not the RWO access mode, is what keeps a run from starting on a volume that is being deleted. The confirmation names what is lost: unpushed commits and uncommitted changes without a WIP snapshot.

### D3 — Provider
`SandboxProvider::delete_task_volume(task: &TaskVolumeSpec)` deletes the PVC by name; Longhorn's reclaim policy removes the backing volume. Kata keeps no state outside the PVC.

### D4 — Design first in PenPot
The boards of task 3.1 are drawn and approved before the UI code changes and are the reference for placement, wording and states. Alternative: implement first and draw afterwards. Rejected: PenPot is the master of the UI design (`docs/design-workflow.md`).

## Risks / Trade-offs

- [A task reopened after its volume was deleted re-clones and rebuilds caches] → the cost of the first run; acceptable at a 7-day grace.
- [Reset while a run is finalizing] → rejected until the run is terminal; the page shows the active run.
- [Task row lock held across a Kubernetes call] → bounded by the 30 s provider timeout, one task at a time; a scheduler tick skips the locked row and starts the task on a later tick.
