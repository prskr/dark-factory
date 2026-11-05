# Tasks

Live and manual checks need a cluster: the dev cluster or the kind cluster of `just e2e-local` (design D5). Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Image volume pull failures (TD-31)

- [ ] 1.1 On the kind cluster of `just e2e-local`, provision a run whose harness image tag does not exist and record the kubelet Events of the pod (`kubectl -n <sandbox ns> get events --field-selector involvedObject.name=<pod> -o yaml`): reason, message and type for the failed image volume pull. Save the event as a test fixture under `crates/df-k8s/src/provider/` and note the reason/message pattern in design D3. Verify: the fixture exists and the pattern in D3 matches it.
- [ ] 1.2 Write failing tests, then map a pod Event of a failed image pull to `SandboxEvent::Gone { reason: "image pull failed: <image>: <message>" }` through the pod store's run-id label, ignoring events of pods not in the store, of pods no longer `Pending`, and unrelated reasons (design D3). Verify with `cargo test -p df-k8s provider` against the fixture of 1.1, a project image event, an unrelated `Scheduled` event, an event of an unknown pod and the fixture event for a `Running` pod (ignored).
- [ ] 1.3 Start the Event watcher (namespaced, field selector `involvedObject.kind=Pod`) in `K8sSandboxProvider::start` and feed its events through `EventTracker`, so a `Gone` from Events and one from the container status are reported once. Verify with an `EventTracker` unit test that emits both and asserts one `Gone`.
- [ ] 1.4 Write a failing test, then add `events` `get`/`list`/`watch` to the verified permissions in `crates/df-k8s/src/startup.rs`. Verify with `cargo test -p df-k8s startup` covering scenario "Missing event permission".
- [ ] 1.5 Add `events` (`get`, `list`, `watch`) to the sandbox namespace Role in `deploy/helm/dark-factory/templates/rbac.yaml` and update the permission table in `deploy/helm/dark-factory/README.md`. Verify with a helm-unittest case in `deploy/helm/dark-factory/tests/control_plane_test.yaml` and `bazel --bazelrc=.bazelrc.agents test //deploy/...`.
- [ ] 1.6 On the kind cluster, repeat the run of 1.1 with the built control plane and check that the run ends `sandbox_lost` with the image in its detail within 30 s of the first pull failure event (scenario "Harness image cannot be pulled"). Record the observed time in this task.

## 2. Unresolvable cluster targets (TD-28, TD-29, TD-32 resolver part)

- [ ] 2.1 Add `ProviderError::Unavailable(String)` to `crates/df-core/src/seams.rs` with a doc comment ("can't create the sandbox right now; retry, the run is fine"). Verify with `cargo build --workspace`.
- [ ] 2.2 Write failing tests, then make `ClusterTargetResolver::resolve` return the rules that failed transiently without a cached resolution next to the targets, and make `get_targets` in `crates/dark-factory/src/sandbox.rs` return `ProviderError::Unavailable` naming them (design D1). Permanent errors and transient errors with a cached resolution behave as today. Verify with `cargo test -p df-k8s provider` (transient without cache, transient with cache, permanent) and `cargo test -p dark-factory sandbox`.
- [ ] 2.3 Write a failing test, then assert in `crates/df-k8s/src/provider/tests.rs` that `provision` creates no pod and no NetworkPolicy when `get_targets` returns `Unavailable`. Verify with `cargo test -p df-k8s provider`.
- [ ] 2.4 Write a failing test, then assert in `crates/df-k8s/src/reconcile/tests.rs` that a run whose targets are `Unavailable` keeps its current NetworkPolicy and the next pass with resolvable targets renders it again (scenario "New leader cannot resolve a target"). Verify with `cargo test -p df-k8s reconcile`.
- [ ] 2.5 Write failing tests, then make `Scheduler::provision_or_fail` log `ProviderError::Unavailable` as a warning and leave the run `provisioning`, with a `ponytail:` comment naming the retry cost of design D2. Verify with scheduler tests (container-backed) using the scripted provider of `df-core::testing`: run stays `provisioning`; a later tick with a succeeding provider creates the sandbox; past the connect timeout the run ends `connect_timeout` (scenarios "Cluster target temporarily unresolvable", "… resolves on a later attempt", "… never resolves").

## 3. Lost sandboxes after a restart (TD-32)

- [ ] 3.1 Write failing tests, then spawn the startup sweep of design D4 into the scheduler's `provisions` `JoinSet` when `Scheduler::run` starts: non-terminal runs in `connecting` or later whose `sandbox_exists` is `false` end `sandbox_lost`; `true` and errors leave the run alone. Verify with container-backed scheduler tests for scenarios "Pod deleted while the control plane was down" and "Pod still there after a restart", plus a run in `provisioning` that the sweep does not touch.
- [ ] 3.2 Update `crates/df-k8s/README.md` ("Sandbox events": image volume pull failures, startup sweep instead of "does not resync"). Verify by reading the section against design D3 and D4.

## 4. Live cluster verification (TD-36, TD-41, TD-59)

- [ ] 4.1 Make `live_config` in `crates/df-k8s/src/test_support.rs` read the runtime image from `DF_LIVE_RUNTIME_IMAGE` and fail with a message naming the variable when unset; extend `test_provider_live_cluster_integration` to assert a `Started` event with a pod IP. Verify by running it with `--ignored` against the kind cluster and recording the result here.
- [ ] 4.2 Add the ClusterIP reachability live test of design D5 (target Service reachable, non-target Service blocked; result read from the probe pod's exit code). Verify by running it with `--ignored` against the kind cluster and recording the result here.
- [ ] 4.3 Run the SNI mismatch check of design D5 from a pod in the sandbox namespace against the deployed gateway. Verify: the tunnel closes before the destination is contacted and the traffic page lists the flow with reason `sni-mismatch`; record command and result here.

## 5. Docs and tech debt

- [ ] 5.1 Update `docs/content/operations/troubleshooting.md`: `sandbox_lost` with "image pull failed" names the runtime, harness or project image; a run that stays `provisioning` while a cluster target can't be resolved (log warning names the target, ends `connect_timeout` if it never resolves); runs whose pod was deleted during a control plane outage end `sandbox_lost` at the next start. Verify with `just docs-build`.
- [ ] 5.2 Delete TD-28, TD-31, TD-32, TD-36, TD-41 and TD-59 (entries, summary rows and the link definition) from `docs/tech_debt.md`. Verify with `grep -n "TD-28\|TD-31\|TD-32\|TD-36\|TD-41\|TD-59\|sandbox-lifecycle-resilience" docs/tech_debt.md` returning only origin lines.

## 6. Integration checks

- [ ] 6.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete` and `just bazel-test`. Verify all pass; list container-backed tests that could not run.

## Workflow follow-up

- On archive, add the backlog row "Live cluster tests in CI" (status *idea*, reference this change) to `product/iterations/roadmap.md`.
