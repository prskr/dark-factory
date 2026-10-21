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
The scheduler leader runs the retention scan once per hour. It selects tasks without a non-terminal run and with `volume_deleted_at` null whose status is `completed` or `cancelled` with the last run older than `volumes.retainAfterClose`, or whose last run is older than `volumes.idleTtl`, and calls `delete_task_volume` for each, then sets `tasks.volume_deleted_at`. `ensure_task_volume` clears the marker when it creates the PVC again. Deleting a PVC that doesn't exist succeeds. *ponytail:* a marker column instead of a volume table; the PVC is the state.

### D2 — Reset action
`POST /tasks/{key}/reset-workspace` rejects tasks with a non-terminal run (409), otherwise calls `delete_task_volume`, sets the marker and writes the audit entry in one transaction after the deletion succeeded. The RWO access mode also protects against a race with a run starting at the same moment: the new pod would wait for the volume, and the scheduler only starts runs for tasks without a non-terminal run, which the reset handler checks under the task row lock. The confirmation names what is lost: unpushed commits and uncommitted changes without a WIP snapshot.

### D3 — Provider
`SandboxProvider::delete_task_volume(task: &TaskVolumeSpec)` deletes the PVC by name; Longhorn's reclaim policy removes the backing volume. Kata keeps no state outside the PVC.

## Risks / Trade-offs

- [A task reopened after its volume was deleted re-clones and rebuilds caches] → the cost of the first run; acceptable at a 7-day grace.
- [Reset while a run is finalizing] → rejected until the run is terminal; the page shows the active run.
