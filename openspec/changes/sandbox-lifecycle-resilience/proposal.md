# Proposal

## Why

Three gaps in the sandbox lifecycle make runs start degraded or wait for a timeout instead of failing fast with a reason: a transient Service lookup at provision time starts the pod without its cluster target (TD-28, TD-29), a runtime or harness image that cannot be pulled leaves the run waiting 10 minutes with no reason (TD-31), and a pod that vanished while the control plane was down is never noticed (TD-32). They need fixing before the first real installation, where control plane restarts, registry hiccups and slow API servers are normal. The provisioning path has also never been exercised end to end on a cluster (TD-36, TD-41, TD-59).

## What Changes

- Provisioning waits for cluster targets: when a cluster target cannot be resolved for a transient reason and no earlier resolution is cached, provisioning does not create the pod. The run stays `provisioning` and the scheduler tries again on its next pass, bounded by the connect timeout.
- Policy reconciliation keeps a run's current NetworkPolicy when a target resolves transiently without a cached resolution (for example on a new leader after a restart), instead of rendering the policy without that target.
- Image volume pull failures end the run: when the kubelet reports that the runtime or harness image of a sandbox pod cannot be pulled, the provider emits `Gone` with a reason naming the image, so the run ends `sandbox_lost` within seconds. The control plane watches pod Events in the sandbox namespace for this; the chart grants and startup verification checks read access to `events`.
- Lost sandboxes are found after a restart: when the scheduler starts, it checks every run that had a sandbox (`connecting` and later, non-terminal) and ends runs whose pod no longer exists as `sandbox_lost`.
- Live cluster verification: a live test that a sandbox with the real runtime image reaches `Started` with its pod IP (TD-36), a live test of ClusterIP reachability for an allowed and a non-allowed Service (TD-41), and a recorded check of the gateway's `sni-mismatch` handling from inside the sandbox namespace (TD-59).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `sandbox-provisioning`: "Sandbox events" also covers image volume pull failures; "Startup verification" checks read access to `events`.
- `run-orchestration`: "Run provisioning" keeps a run in `provisioning` while a cluster target is temporarily unresolvable; "Timeouts and lost sandboxes" ends runs whose pod vanished while no control plane was watching.
- `run-network-policy`: "Policies follow the database" keeps a run's current policy while a target is temporarily unresolvable and has no earlier resolution.

## Out of scope

- A deadline for pods that stay `Pending` for other reasons (unschedulable, volume attach): the connect timeout covers them. Rejected for now; revisit if `Pending` pods become a frequent support question.
- Persisting the resolver's last good resolutions in the database: keeping the current policy covers the restart case. Rejected.
- Running the live tests in CI against a kind cluster: backlog of `product/iterations/roadmap.md` ("Live cluster tests in CI"; the row is added when this change is archived).

## Impact

- `crates/df-k8s`: `ClusterTargetResolver::resolve` reports unresolved targets; `K8sSandboxProvider::provision` returns a new retryable error; reconcile skips runs with unresolved targets; a pod Event watcher feeds `EventTracker`; startup verification lists `events`; live tests.
- `crates/df-core`: new `ProviderError::Unavailable` variant (`seams.rs`).
- `crates/df-agent-channel`: scheduler keeps a run `provisioning` on `Unavailable`; startup sweep of lost sandboxes.
- `deploy/helm/dark-factory`: `events` get/list/watch in the sandbox namespace Role; chart README permission table.
- Docs: `docs/content/operations/troubleshooting.md`.
- Tech debt: deletes TD-28, TD-31, TD-32, TD-36, TD-41, TD-59 from `docs/tech_debt.md`.
