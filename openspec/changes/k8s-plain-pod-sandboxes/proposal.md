# Proposal

## Why

The Kubernetes provider wraps each run's pod in an agent-sandbox `Sandbox`, but uses none of its features: it only sets `podTemplate`, `operatingMode: Running` and `shutdownPolicy: Retain`, never reads `SandboxStatus` (run state comes from the Pod watcher), and the design rules out warm pools, claims, templates, suspend/resume and `shutdownTime` (ADR-0022). The wrapper costs a cluster prerequisite (CRDs and controller), 4,750 lines of kopium-generated types that must follow the pinned release (the generated module still names `v0.5.0`/`v1alpha1` while e2e installs `v1.0.9` and the code uses `v1beta1`), a second watcher, an ownership hop in identity resolution and extra RBAC. Pre-MVP is the time to drop it, and doing so first leaves a smaller Kubernetes provider before the `SandboxProvider` seam is generalized for VM providers (Proxmox).

## What Changes

- **BREAKING** The control plane creates one plain `Pod` per run (`restartPolicy: Never`) instead of an agent-sandbox `Sandbox`. Pod name, labels, volumes, security context and runtime class stay the same.
- **BREAKING** agent-sandbox is no longer a cluster prerequisite. Existing `Sandbox` objects are not migrated (no installations exist).
- The per-run `NetworkPolicy` is owned by the run's Pod; deleting the Pod garbage-collects the policy.
- Identity resolution checks the Pod's own labels (instance, run id) instead of an owning `Sandbox`.
- A pod whose container exits without a run report (phase `Succeeded`) is reported as gone, like a `Failed` pod.
- Teardown and existence checks act on the Pod; the policy reconcile loop lists the instance's Pods.
- RBAC: the control plane gets `pods` create/delete in the sandbox namespace; the `agents.x-k8s.io` rule and its startup checks are removed.
- Removed: `crates/df-k8s/src/crd.rs` (and its file-length allow entry), the Sandbox watcher, the agent-sandbox install in `deploy/e2e/e2e-local.sh`.
- Docs: ADR-0030 amended (Kubernetes provider uses plain Pods), ADR-0001/0003, `design/tech-stack.md`, `design/deployment.md`, `design/agent-run.md`, `product/context.md`, `docs/demo.md`, `crates/df-k8s/README.md`; the roadmap backlog row "Replace agent-sandbox with plain scheduled Pods" is removed.

Out of scope:
- Generalizing `SandboxProvider` for VM providers and a Proxmox provider: follow-up, to be added to the backlog in `product/iterations/roadmap.md` from the handoff issue.
- `activeDeadlineSeconds` as a safety net when the control plane is down: rejected, the wall-clock budget and the reconcile on restart already cover it.
- A migration of existing `Sandbox` objects: rejected, pre-MVP.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `sandbox-provisioning`: a run is a Pod (not a `Sandbox`) with `restartPolicy: Never`; teardown deletes the Pod; a completed pod is reported as gone; startup verification checks Pod permissions.
- `sandbox-identity`: the pod itself must carry the instance and run id labels; no owning `Sandbox`.
- `run-network-policy`: the per-run policy is owned by the run's Pod.

## Impact

- Code: `crates/df-k8s` (`render`, `provider`, `reconcile`, `startup`, `crd` removed, tests and snapshots), doc comments in `crates/df-core/src/seams.rs` and `crates/dark-factory/src/sandbox.rs`, root `Cargo.toml` (file-length allow entry).
- Deployment: Helm RBAC template and `tests/rbac_test.yaml`; `deploy/e2e/e2e-local.sh`.
- Dependencies: none added; `kopium` regeneration steps disappear.
- Clusters: agent-sandbox can be uninstalled; Kubernetes 1.36+, cert-manager csi-driver and a NetworkPolicy-enforcing CNI remain required.
