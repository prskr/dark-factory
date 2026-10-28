# Proposal

## Why

Every task gets a Longhorn volume with its clone and build caches, and nothing ever deletes it: `design/agent-run.md` §5.5 describes retention and "reset workspace", but `iter1-sandbox-provisioning` lists both as non-goals and no later change picks them up. In a homelab the volumes of finished tasks become the largest consumer of storage. There is also no way out when a volume is broken (its node is gone, the workspace diverged, a build cache is corrupt) other than editing Kubernetes by hand.

## What Changes

- **Retention:** the leader deletes the task volume of tasks that have been `completed` or `cancelled` for longer than `volumes.retainAfterClose` (default 7 days), and of any task without an active run whose last run ended more than `volumes.idleTtl` (default 30 days) ago. The next run of such a task re-clones (`workspace-lifecycle`, "Volume lost after earlier runs").
- **Reset workspace:** a task detail action (with confirmation) that deletes the volume of a task without an active run. Audited as `task.reset_workspace`.
- **Provider:** `SandboxProvider::delete_task_volume`, idempotent.
- **PenPot:** the task detail boards (Desktop, Phone) get the reset action and a confirmation board before the code changes.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `sandbox-provisioning` (introduced by `iter1-sandbox-provisioning`): deleting a task volume, recreation on the next run.
- `task-management`: volume retention and the reset action.
- `task-ui` (introduced by `iter1-control-plane-web`): the reset action.

## Impact

- Code: `SandboxProvider` trait and `df-k8s` (PVC deletion), `df-db` migration (`tasks.volume_deleted_at`), a leader-driven hourly scan in `df-agent-channel`, `df-web` action. Instance configuration `volumes.retainAfterClose`, `volumes.idleTtl`.
- No contract change.
- **Depends on** `iter1-sandbox-provisioning` (archived first: adds requirements to `sandbox-provisioning`) and `iter2-cancel` (status `cancelled`).
- PenPot file "App Design": task detail boards (Desktop, Phone), reset confirmation board; mapping rows in `docs/design-workflow.md`.
