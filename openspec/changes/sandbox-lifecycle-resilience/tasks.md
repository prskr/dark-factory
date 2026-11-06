# Tasks

Live and manual checks need a cluster: the dev cluster or the kind cluster of `just e2e-local` (design D5). Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Image volume pull failures (TD-31)

- [x] 1.1 On the kind cluster of `just e2e-local`, provision a run whose harness image tag does not exist and record the kubelet Events of the pod (`kubectl -n <sandbox ns> get events --field-selector involvedObject.name=<pod> -o yaml`): reason, message and type for the failed image volume pull. Save the event as a test fixture under `crates/df-k8s/src/provider/` and note the reason/message pattern in design D3. Verify: the fixture exists and the pattern in D3 matches it.
  - Recorded 2026-11-06 with a pod shaped like a sandbox pod (runtime container, harness image volume `claude-harness:does-not-exist`) on kind/containerd, Kubernetes 1.37.0, and again on 1.36.1. Fixtures: `crates/df-k8s/src/provider/fixtures/image_volume_pull_failed_{events,pod}.json`. Finding: the pod status also reports `waiting.reason: ErrImagePull`, so `map_pod_event` already covers this; the Event watcher stays as defense in depth (design D3).
- [x] 1.2 Write failing tests, then map a pod Event of a failed image pull to `SandboxEvent::Gone { reason: "image pull failed: <image>: <message>" }` through the pod store's run-id label, ignoring events of pods not in the store, of pods no longer `Pending`, and unrelated reasons (design D3). Verify with `cargo test -p df-k8s provider` against the fixture of 1.1, a project image event, an unrelated `Scheduled` event, an event of an unknown pod and the fixture event for a `Running` pod (ignored), plus a `map_pod_event` test on the recorded pod status that expects `Gone` naming the image.
- [x] 1.3 Start the Event watcher (namespaced, field selector `involvedObject.kind=Pod`) in `K8sSandboxProvider::start` and feed its events through `EventTracker`, so a `Gone` from Events and one from the container status are reported once. Verify with an `EventTracker` unit test that emits both and asserts one `Gone`.
- [x] 1.4 Write a failing test, then add `events` `get`/`list`/`watch` to the verified permissions in `crates/df-k8s/src/startup.rs`. Verify with `cargo test -p df-k8s startup` covering scenario "Missing event permission".
- [x] 1.5 Add `events` (`get`, `list`, `watch`) to the sandbox namespace Role in `deploy/helm/dark-factory/templates/rbac.yaml`. The chart README has no permission table; the permission lists in `design/deployment.md` §3 and `docs/content/architecture/_index.md` were updated instead. Verified with a helm-unittest case in `deploy/helm/dark-factory/tests/rbac_test.yaml` and `bazel --bazelrc=.bazelrc.agents test //deploy/...`.
- [x] 1.6 On the kind cluster, repeat the run of 1.1 with the built control plane and check that the run ends `sandbox_lost` with the image in its detail within 30 s of the first pull failure event (scenario "Harness image cannot be pulled"). Record the observed time in this task.
  - 2026-11-06, kind (Kubernetes 1.37.0), control plane from iteration 7. The e2e projects set an adapter command and mount no harness volume, so the runtime image volume was broken instead (`image.runtime.tag=does-not-exist`; same image volume path). Two runs started through `lifecycle_events`: first `Failed to pull image` Event at 21:10:28, run ended `sandbox_lost` at 21:10:28.37; second attempt Event at 21:12:30, ended at 21:12:30.75. Both well under 1 s. Detail: `sandbox is gone: image pull failed: ghcr.io/prskr/dark-factory/runtime:does-not-exist: rpc error: code = NotFound …` (the Event watcher reported first).

## 2. Unresolvable cluster targets (TD-28, TD-29, TD-32 resolver part)

- [x] 2.1 Add `ProviderError::Unavailable(String)` to `crates/df-core/src/seams.rs` with a doc comment ("can't create the sandbox right now; retry, the run is fine"). Verify with `cargo build --workspace`.
- [x] 2.2 Write failing tests, then make `ClusterTargetResolver::resolve` return the rules that failed transiently without a cached resolution next to the targets, and make `get_targets` in `crates/dark-factory/src/sandbox.rs` return `ProviderError::Unavailable` naming them (design D1). Permanent errors and transient errors with a cached resolution behave as today. Verify with `cargo test -p df-k8s provider` (transient without cache, transient with cache, permanent) and `cargo test -p dark-factory sandbox`.
- [x] 2.3 Write a failing test, then assert in `crates/df-k8s/src/provider/tests.rs` that `provision` creates no pod and no NetworkPolicy when `get_targets` returns `Unavailable`. Verify with `cargo test -p df-k8s provider`.
- [x] 2.4 Write a failing test, then assert in `crates/df-k8s/src/reconcile/tests.rs` that a run whose targets are `Unavailable` keeps its current NetworkPolicy and the next pass with resolvable targets renders it again (scenario "New leader cannot resolve a target"). Verify with `cargo test -p df-k8s reconcile`.
- [x] 2.5 Write failing tests, then make `Scheduler::provision_or_fail` log `ProviderError::Unavailable` as a warning and leave the run `provisioning`, with a `ponytail:` comment naming the retry cost of design D2. Verify with scheduler tests (container-backed) using the scripted provider of `df-core::testing`: run stays `provisioning`; a later tick with a succeeding provider creates the sandbox; past the connect timeout the run ends `connect_timeout` (scenarios "Cluster target temporarily unresolvable", "… resolves on a later attempt", "… never resolves").

## 3. Lost sandboxes after a restart (TD-32)

- [x] 3.1 Write failing tests, then spawn the startup sweep of design D4 into the scheduler's `background` `JoinSet` when `Scheduler::run` starts: non-terminal runs in `connecting` or later whose `sandbox_exists` is `false` end `sandbox_lost`; `true` and errors leave the run alone. Verify with container-backed scheduler tests for scenarios "Pod deleted while the control plane was down" and "Pod still there after a restart", plus a run in `provisioning` that the sweep does not touch.
- [x] 3.2 Update `crates/df-k8s/README.md` ("Sandbox events": image volume pull failures, startup sweep instead of "does not resync"). Verify by reading the section against design D3 and D4.

## 4. Live cluster verification (TD-36, TD-41, TD-59)

- [x] 4.1 Make `live_config` in `crates/df-k8s/src/test_support.rs` read the runtime image from `DF_LIVE_RUNTIME_IMAGE` and fail with a message naming the variable when unset; extend `test_provider_live_cluster_integration` to assert a `Started` event with a pod IP. Verify by running it with `--ignored` against the kind cluster and recording the result here.
  - 2026-11-06, kind `df-e2e` (Kubernetes 1.37.0, kindnet enforces NetworkPolicy), release in `dark-factory`, `DF_LIVE_RUNTIME_IMAGE=ghcr.io/prskr/dark-factory/runtime:e2e`, empty storage and runtime class. `test_provider_live_cluster_integration` passed in 7.1 s: the pod watcher reported `Started` with the pod IP. All three live tests (with `test_reconciliation_live_cluster` and `policy_loop_live_reverts_manual_edit_and_adds_new_target`) pass with `--test-threads=1` (65 s); run in parallel they interfere in the shared sandbox namespace (provider test timed out, a reconcile assertion failed), so the README requires one thread. `live_config` also reads the release namespace and storage/runtime class from `DF_LIVE_*` (dev-cluster defaults) and looks up the control plane and gateway Service IPs.
- [x] 4.2 Add the ClusterIP reachability live test of design D5 (target Service reachable, non-target Service blocked; result read from the probe pod's exit code). Verify by running it with `--ignored` against the kind cluster and recording the result here.
  - 2026-11-06, same cluster: `sandbox_reaches_the_target_cluster_ip_and_not_another_service` passed in 25 s. The probe pod (run-labelled, sandbox namespace) exits `10 + 1·target reachable + 2·other reachable`: 11, so only the target ClusterIP was reachable. A control pod in the Services' namespace exited 13 (both reachable), so the block comes from the NetworkPolicy. Services, pods and the policy are removed before the assertions; no leftovers in `dark-factory-sandboxes` afterwards.
- [x] 4.3 Run the SNI mismatch check of design D5 from a pod in the sandbox namespace against the deployed gateway. Verify: the tunnel closes before the destination is contacted and the traffic page lists the flow with reason `sni-mismatch`; record command and result here.
  - Kind `df-e2e` (Kubernetes 1.37.0). A temporary instance allowlist entry `example.com:443` was added to the database (removed afterwards), and a run of project NA (`run-na-1-1`) was started.
  - Command in its container: `echo | timeout 20 openssl s_client -proxy "${HTTPS_PROXY#http://}" -connect example.com:443 -servername other.example.net`.
  - Result: `CONNECTED`, then `unexpected eof while reading`, `no peer certificate available`, 56 bytes read. The gateway closed the tunnel before a server answered.
  - Control run with `-servername example.com`: `subject=CN=example.com`, `Verify return code: 0`.
  - Egress log (`/egress/log`): `21:32:13 NA-1 #1 example.com:443 Blocked sni-mismatch` next to the allowed control flow.

## 5. Docs and tech debt

- [x] 5.1 Update `docs/content/operations/troubleshooting.md`: `sandbox_lost` with "image pull failed" names the runtime, harness or project image; a run that stays `provisioning` while a cluster target can't be resolved (log warning names the target, ends `connect_timeout` if it never resolves); runs whose pod was deleted during a control plane outage end `sandbox_lost` at the next start. Verify with `just docs-build`.
  - Verified: the `docs-build` recipe steps (`build/with_tools.sh docs/publish.sh target/docs-site main`) build without errors; `docs/links_test.sh` passes.
- [x] 5.2 Delete TD-28, TD-29, TD-31, TD-32, TD-36, TD-41 and TD-59 (entries, summary rows and the link definition) from `docs/tech_debt.md`. Verify with `grep -n "TD-28\|TD-29\|TD-31\|TD-32\|TD-36\|TD-41\|TD-59\|sandbox-lifecycle-resilience" docs/tech_debt.md` returning only origin lines.
  - Verified: the grep returns only the origin lines 10, 11 and 22. Outside `docs/tech_debt.md`, only `product/iterations/iteration-3.md` names the IDs (iteration plan, no links).

## 6. Integration checks

- [x] 6.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete` and `just bazel-test`. Verify all pass; list container-backed tests that could not run.
  - 2026-11-06 on macOS (arm64) with a container socket (OrbStack):
    - clippy: no warnings.
    - `cargo machete`: no unused dependencies.
    - `cargo xtask check-test-names` and `check-crates`: ok.
  - `bazel --bazelrc=.bazelrc.agents test //... --keep_going`: 46 tests pass and 5 are skipped. All container-backed tests ran.
    - The skipped tests depend on `//deploy/images/{claude,opencode}-harness:layer`. They don't load on macOS: `musl_runtime_linux_*` cannot be extracted, and there is no patchelf toolchain for the host.
    - This change does not touch those targets, and CI (Linux) builds them.

## Workflow follow-up

- On archive, add the backlog row "Live cluster tests in CI" (status *idea*, reference this change) to `product/iterations/roadmap.md`.
