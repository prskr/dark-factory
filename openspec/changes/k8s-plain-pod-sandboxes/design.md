# Design

## Context

See proposal.md (Why). Today `df-k8s`:

- renders a `Sandbox` (`render_sandbox`) whose `podTemplate` carries the full pod spec, and applies it with server-side apply;
- watches Pods and Sandboxes (two reflectors, merged in `forward_events`); run state comes only from the Pod watcher, the Sandbox watcher contributes "sandbox deleted" events;
- resolves identity through Pod → owner reference → Sandbox (UID check) → Sandbox labels;
- makes the per-run `NetworkPolicy` owned by the Sandbox, and the leader's reconcile loop lists Sandboxes to re-render policies;
- checks six `agents.x-k8s.io` permissions at startup.

The `SandboxProvider` trait (`df-core::seams`) does not change; everything below is internal to `df-k8s` plus RBAC, e2e setup and docs.

## Goals / Non-Goals

**Goals:**
- One Pod per run, same name (`run-<key>-<n>`), labels, volumes and security settings as the pod agent-sandbox created.
- One watcher (Pods) as the only source of `Started`/`Gone`.
- No agent-sandbox types, permissions or install steps left in the repository.

**Non-Goals:**
- Renaming `SandboxProvider`, `SandboxEvent`, `RunSandboxSpec`: rejected, "sandbox" stays the provider-neutral domain term (`design/agent-run.md` glossary), which a VM provider will reuse.
- Generalizing the provider seam (identity, egress, volumes) for VM providers such as Proxmox: backlog in `product/iterations/roadmap.md`, from the Proxmox handoff issue.
- `activeDeadlineSeconds` or other pod-level time limits: rejected, the run budget and stale-run handling in the control plane already cover it.

## Decisions

### D1 Create the Pod, treat `AlreadyExists` as success
`provision` sends a plain create for the Pod; HTTP 409 `AlreadyExists` counts as success (spec "Provisioning again is idempotent"). The NetworkPolicy keeps using server-side apply.

- Why: a Pod spec is immutable. Server-side apply of a re-rendered pod whose config changed between two attempts (new harness image, new cluster target) is rejected with 422, which would fail the run with `provisioning_failed` instead of keeping the pod that already runs.
- Alternative: server-side apply as today. Works only while the rendered pod is byte-identical.
- Quota rejections (403 `exceeded quota`) still map to `ProviderError::Capacity`; the existing `is_quota_rejection` check applies unchanged to Pod creation.

### D2 `restartPolicy: Never`; a completed pod is gone
A run is one attempt: the runtime holds per-run state in `/run/factory` (tmpfs) and the scheduler owns retries ("Automatic resume after infrastructure failures"). `map_pod_event` adds phase `Succeeded` → `Gone { reason: "pod completed" }`, next to `Failed`. Normally the `RunReport` arrives first and the run is already terminal, so the event is ignored; without a report the run fails as `sandbox_lost` immediately instead of waiting for a timeout.

- Alternative: `OnFailure`. A restarted container would reconnect with the same certificate and pod IP and start a second runtime on a half-written workspace; rejected.

### D3 NetworkPolicy owned by the Pod
`render_network_policy` takes the Pod's name and UID for the owner reference (`apiVersion: v1`, `kind: Pod`, `controller: true`). `blockOwnerDeletion` is left unset: setting it requires `update` on `pods/finalizers` where the `OwnerReferencesPermissionEnforcement` admission plugin is enabled, and the control plane never uses foreground deletion. The order stays: create the Pod, then the policy (the UID is needed); until the policy exists the namespace's default-deny baseline applies, as today.

### D4 Identity from Pod labels
`resolve_identity` drops the Sandbox store: after the namespace, UID and IP checks it requires `dark-factory.io/instance=<instance>` and a valid `dark-factory.io/run-id` on the pod itself. The pod reflector already filters by the instance label, so the label check is a second line of defence against a stale cache entry, not a new trust boundary. Anyone able to create labelled pods in the sandbox namespace could already create labelled Sandboxes before; RBAC keeps that limited to the control plane.

### D5 One watcher
`K8sSandboxProvider::start` keeps only the Pod reflector. `forward_events` takes a single stream; the relist logic (`Init` .. `InitDone`, no `Delete` during relist, "gone" for runs missing after a relist) stays and is driven by pods alone. `sandbox_exists` and `teardown` list Pods by `instance` + `run-id` labels instead of Sandboxes; teardown deletes them with default (background) propagation, which removes the policy through D3.

### D6 Reconcile loop lists Pods
`list_instance_sandboxes` becomes a Pod list with the instance label; `render_run_policy` reads labels, name and UID from the Pod. Pods with a deletion timestamp are skipped (the policy goes with them). Terminal pods that are not yet torn down keep their policy, as Sandboxes did.

### D7 RBAC
Sandbox namespace Role: `pods` create/get/list/watch/delete (previously get/list/watch); the `agents.x-k8s.io` rule is removed; no `patch`/`update` on pods (D1). `SANDBOX_PERMISSIONS` changes accordingly. Granting `pods` create is no privilege gain: the control plane could already create Sandboxes with arbitrary pod templates, Pod Security Admission `restricted` on the namespace still applies, and the agent-sandbox controller's cluster-wide pod rights disappear.

### D8 Rendering types
Pods use `k8s_openapi::api::core::v1::{Pod, PodSpec, Container, Volume, …}` directly instead of the kopium `SandboxPodTemplate*` mirrors; `crd.rs` and its `[workspace.metadata.file-length.allow]` entry are removed. Render snapshots (`render/snapshots/*.snap`) change from a `Sandbox` to a `Pod`; review them for unchanged pod fields.

## Risks / Trade-offs

- [A cluster still running the agent-sandbox controller with old Sandboxes would recreate pods the control plane deletes] → Migration plan: delete old Sandboxes before upgrading; pre-MVP, only the dev cluster is affected.
- [Pod create with 409 hides a pod left over from an earlier attempt with a different spec] → Same run, same name; `sandbox_exists` guards re-provisioning and teardown deletes the pod before a new run number is used, so a stale pod only lives as long as its run.
- [Node failure: a plain Pod is never rescheduled] → Intended: the pod becomes `Failed`/deleted, the run fails `sandbox_lost` and the scheduler resumes the task on a new pod.
- [Snapshot and live tests (`policy_loop_live`, ignored) need the dev cluster] → Run them on the dev cluster after the change; CI covers the rest.

## Migration Plan

1. On the dev cluster: wait for or stop active runs, `kubectl -n <sandbox ns> delete sandboxes.agents.x-k8s.io -l dark-factory.io/instance=<instance>`.
2. Upgrade the chart (new Role). Task PVCs are untouched; the next run of each task mounts its PVC as before.
3. Optionally uninstall agent-sandbox (controller and CRDs).

Rollback: reinstall agent-sandbox and the previous chart; no data migration is involved in either direction.
