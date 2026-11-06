# Design

## Context

See proposal.md (Why) and `docs/tech_debt.md` (TD-28, TD-31, TD-32, TD-36, TD-41, TD-59) for the problems.

Current state in code:

- `ClusterTargetResolver::resolve` (`crates/df-k8s/src/provider.rs`) keeps a per-process "last good" cache per scope. A transient lookup error without a cached entry skips the rule with a warning. Both `K8sSandboxProvider::provision` and the leader's `render_run_policy` (`crates/df-k8s/src/reconcile.rs`) get targets through `ClusterTargetsProvider::get_targets` (`crates/dark-factory/src/sandbox.rs`). On an `Err`, reconcile already skips the run and keeps its policy.
- `Scheduler::provision_or_fail` (`crates/df-agent-channel/src/scheduler.rs`) ends the run `provisioning_failed` on any provider error. `reprovision_stale` provisions runs again that stay `pending`/`provisioning` for more than 60 s, every 5 s tick, unless `sandbox_exists`.
- `map_pod_event` maps container `waiting` reasons (`ErrImagePull`, `ImagePullBackOff`, `CreateContainerConfigError`) to `Gone`. TD-31 assumed that a failed image volume pull leaves the containers in `ContainerCreating`. Task 1.1 disproved this on kind (containerd, Kubernetes 1.36.1 and 1.37.0): the pod stays `Pending` and the container's `waiting.reason` is `ErrImagePull`, with the image reference in the message. The container-status check already ends such a run.
- `EventTracker` reports a vanished pod only if this process saw it before. After a restart, a pod deleted while no replica ran is never reported. The scheduler runs on every replica; reconcile runs on the leader only.

## Goals / Non-Goals

**Goals:**
- Fix the three behaviors with the smallest change at the seams that already exist (`get_targets`, `SandboxEvent`, `sandbox_exists`).
- No new run outcome, no new configuration option.

**Non-Goals:**
- Generic `Pending` deadline (rejected, see proposal).
- Persisting resolver state (rejected, see proposal).
- Live tests in CI (backlog, see proposal).
- Making the per-attempt retry cheaper (D2 ceiling) — not needed at today's scale. Rejected until a cluster target outage with many queued runs shows API pressure.

## Decisions

### D1: An unresolvable target fails `get_targets`, not `resolve`'s caller

`ClusterTargetResolver::resolve` returns the targets plus the rules that failed transiently without a cached resolution. `get_targets` turns a non-empty list into `Err(ProviderError::Unavailable(..))` naming the rules. A new `ProviderError::Unavailable` variant in `df-core` means "the provider can't create the sandbox right now; try again, the run is fine".

Both callers then behave correctly without new branches: `provision` propagates the error before creating the pod (TD-28, and the named-`targetPort` case formerly TD-29), and reconcile's existing `Err` branch keeps the run's current policy (TD-32, resolver part). Permanent errors keep evicting the rule as today.

Alternative: return the partial target list and let each caller check a flag. Rejected: two call sites must remember the check; with the error the type system enforces it.

### D2: The scheduler leaves an `Unavailable` run in `provisioning`

`provision_or_fail` logs `Unavailable` as a warning and returns without ending the run. The run is already `provisioning` (`begin_provisioning` ran), so `reprovision_stale` retries it once it is older than 60 s, on every tick, until a pod exists or `enforce_timeouts` ends it with `connect_timeout` (spec: "Cluster target never resolves").

Alternative: end the run `provisioning_failed` and requeue like `capacity`. Rejected: every attempt would create a new run and emit lifecycle events (`sandbox.provisioning_failed`) to receivers, and the run list fills with failed runs for a hiccup.

Ceiling: a stuck run costs one `sandbox_exists` list plus up to two lookups per rule every 5 s. Code gets a `ponytail:` comment naming it.

A tick starts no second attempt for a run whose earlier attempt on this replica is still in flight (an in-memory claim per run id, released when the attempt ends). Otherwise a slow `Unavailable` attempt and the 5 s tick stack attempts. The claim only covers one replica; another replica may still retry the same run, which is harmless because volume and sandbox creation are idempotent.

On a retry by `reprovision_stale`, `Unavailable` and `Backend` errors are only logged: a timed-out create may still have succeeded, and the Kubernetes provider maps both a rejected pod create and a timeout to `Backend`. `Capacity` and `InvalidSpec` end the run `provisioning_failed` like a first attempt (a capacity rejection requeues the task without counting a resume), unless a second `sandbox_exists` finds the sandbox by now.

### D3: Image volume pull failures also come from pod Events

On containerd with Kubernetes 1.36 and 1.37 the container status already reports the failed pull (Context). The Event watcher is defense in depth for runtimes that report it only as an Event. The project decided to keep it despite the extra RBAC.

The provider starts a second watcher on core `v1` `Event`s in the sandbox namespace, field selector `involvedObject.kind=Pod`. An event whose reason and message say an image pull failed is mapped to the run through the pod's `dark-factory.io/run-id` label in the existing pod store, and fed into `EventTracker` as `Gone { reason: "image pull failed: <ref>: <message>" }`. Events of pods not in the store are ignored, and so are events of a pod whose phase is no longer `Pending`: Kubernetes keeps Events for an hour, and a watcher start or relist lists them again, so a pod that started after a failed pull attempt must not end as `sandbox_lost`. `EventTracker::dedupe` already drops a second `Gone`.

Recorded pattern (task 1.1, fixture `crates/df-k8s/src/provider/fixtures/image_volume_pull_failed_events.json`): `type: Warning`, `reason: Failed`, `source.component: kubelet`, message `Failed to pull image "<ref>": <runtime error>`. The same pull also produces `Failed` events with messages `Error: ErrImagePull` and `Error: ImagePullBackOff`; they name no image and are ignored. Only the message prefix `Failed to pull image "` matches, and the image is the quoted reference after it. `<message>` in the `Gone` reason is the runtime error after `"<ref>": `. An event whose `involvedObject.uid` differs from the stored pod's UID belongs to an earlier pod with the same name and is ignored (`map_image_pull_event` in `provider/pull_events.rs`).

This also covers the project image, but `map_pod_event` keeps its container-status check: whichever comes first wins.

The Role gets `events` `get`, `list`, `watch` in the sandbox namespace, and `startup.rs` adds them to the verified permissions.

Alternatives:
- A deadline for pods stuck in `ContainerCreating`: still waits minutes and has no reason to show. Rejected.
- Pod conditions: `PodReadyToStartContainers=false` names no image. Rejected.

### D4: The scheduler sweeps runs with a sandbox once at startup

When `Scheduler::run` starts, it spawns one task into its existing `background` `JoinSet`: list the non-terminal runs in `connecting` or later (a sandbox was reported), call `sandbox_exists` for each, and `finish_run` with `sandbox_lost` (`"sandbox pod no longer present after a control plane restart"`) when it returns `false`. The reason names no pod: the scheduler sees the provider only through `SandboxProvider`, and the run id already identifies the pod (label `dark-factory.io/run-id`). Up to 8 checks run at once. A failed listing or check is logged and the whole pass is retried one tick interval later, 3 passes in all; runs still unchecked then end through the usual timeouts, and an error is logged.

Every replica sweeps; `finish_run` is guarded by the run state, so two replicas ending the same run is harmless. Runs in `pending`/`provisioning` are left to `reprovision_stale`.

Alternatives:
- Seed `EventTracker` with the active runs from the database: puts database knowledge into `df-k8s`, which the `SandboxProvider` seam avoids. Rejected.
- A periodic sweep: the watcher's relist already reports pods that vanished while this process ran; only the restart gap is open. Rejected.

### D5: Live tests run against any cluster with the sandbox namespace

The live tests stay `#[ignore]` and need a cluster: the dev cluster, or the kind cluster of `just e2e-local` (`deploy/e2e/kind.yaml`; kindnet enforces `NetworkPolicy`).

- TD-36: `test_support::live_config` reads the runtime image from `DF_LIVE_RUNTIME_IMAGE` and fails with a message naming the variable when it is unset; `alpine` goes away. The provider live test asserts a `Started` event with a pod IP.
  - Further settings: `DF_LIVE_INSTANCE_NAMESPACE` (default `dark-factory-dev`), `DF_LIVE_STORAGE_CLASS` (default `longhorn`, empty = cluster default), `DF_LIVE_RUNTIME_CLASS` (default `kata`, empty = none). The cluster IPs of the `dark-factory-control-plane` and `dark-factory-gateway` Services in the instance namespace are looked up.
- TD-41: a live test creates a target Service with one pod and a second, non-target Service, provisions the run's policy with only the first as target, and starts a pod with the run's labels whose command connects to both ClusterIPs (`nc -z -w 3`) and exits with a code encoding both results. The test reads the pod's exit code; no `exec` support is needed.
- TD-59: a manual check from a pod in the sandbox namespace: `openssl s_client -proxy <gateway>:3128 -connect <allowlisted host>:443 -servername <other name>`. The tunnel must close before the destination is contacted and the traffic page must show `sni-mismatch`. The result is recorded in tasks.md.

## Risks / Trade-offs

- [Kubelet event text differs between versions or for image volumes] → Task 1.1 records the real reason and message on a cluster first; the matcher is tested against the recorded event.
- [A transient registry error is reported as `Failed` once and the kubelet would succeed on retry] → The run ends `sandbox_lost` and is resumed automatically (counts against `runs.maxAutoResumes`). The same holds today for the project image.
- [The startup sweep lists many runs on a big instance] → One list call per active run with a sandbox, in the background; acceptable for hundreds of runs.
- [The event watcher adds API watch load] → One namespaced watch with a field selector, same as the pod watcher.

## Migration Plan

The chart adds `events` to the sandbox namespace Role. A control plane deployed without the updated Role refuses to start and names the permission (spec "Missing event permission"). Pre-MVP: no compatibility path. Rollback: previous chart and image.
