# Review: iter1/sandbox-provisioning (uncommitted changes)

- **Scope:** `git diff HEAD` plus the untracked `crates/df-k8s/` and `crates/df-db/src/egress.rs`. Lock files were not reviewed.
- **Spec:** `openspec/changes/iter1-sandbox-provisioning/` (proposal, design, tasks, specs).
- **Round 3 (re-review):** every "FIXED" claim from the fixer's round-3 notes was checked against the code. Each one is marked **HOLDS**, **PARTIAL**, **NOT FIXED** or **CLAIM WRONG**. "Claim wrong" means the fix note describes code that doesn't exist; the code itself may still be fine.
- Items marked ✔ were re-checked by hand against the source.

## Verification status

The fixer reported these checks passing: fmt, clippy, machete, deny, helm lint, bazel build and test, and `cargo llvm-cov -p df-k8s` at 72.98 % lines. The review sandbox has no `rustup` and no access to crates.io (403 on `index.crates.io`), so **none of them could be reproduced**. They are claimed, not verified.

Not run at all:
- Container-backed tests (the `egress.rs` test).
- Dev-cluster tests (tasks 1.x, 3.1, 4.1, 4.2, 5.1).
- Helm unit tests (`helm unittest`): the fixer ran only `helm lint`, and the unit tests will fail (R3-B3).

## Summary

| | Open blocking | Open non-blocking |
|---|---|---|
| Earlier findings, not or only partly fixed | 4 (items 3, 6, 11, R2-B6) | ~10 |
| New in round 3 | 5 (R3-B1 resolved by decision) | 9 |

Worst open issues:
- **Spec:** the new ClusterIssuer can't find its CA Secret, so **no certificate is issued anywhere**, including for the control plane and gateway (R3-B2).
- **Standards:** the spin regression test passes without the fix (R3-B6). Also, defaults still exist in three or more places, and they now disagree (round-1 item 11).

---

## Earlier findings: validation

### Round 1
1. **Reconcile loop spin.** **PARTIAL.**
   - The `notify_active` guard holds (`reconcile.rs:395,416,431`).
   - The forwarder is now owned (JoinSet with `until_stopped`), handles `Lagged`, and uses `try_send`.
   - Its regression test does not fail without the fix; see R3-B6.
   - `shutdown.changed()` and `is_leader.changed()` ignore `Err`; see R3-N1.
2. **DB-backed cluster targets.** **HOLDS** (`DbClusterTargetsProvider` → `df_db::egress::list_cluster_services`).
3. **Policies follow Service/EndpointSlice changes.** ✔ **NOT FIXED.**
   - `slice_watch_task` (`main.rs:396-398`) watches EndpointSlices only in `cfg.sandbox.instance_namespace`, the release namespace.
   - Target Services live in `clusterTargets.namespaces` (or anywhere with `anyNamespace`). Nothing watches those namespaces, and nothing watches Services.
   - Watch errors are dropped (`if let Ok(..)`).
   - Design D5: "Service/EndpointSlice watch events in cluster-target namespaces". *Blocking.*
4. **Reconcile uses real labels.** **HOLDS, CLAIM WRONG.**
   - The code reads `dark-factory.io/run-id`, `task-key` and `project` from **Sandbox** labels (`reconcile.rs:221-257`), matching `render.rs:129-145`, and logs `warn!` when a label is missing.
   - The fix note's "Pod labels `project-key`, `task-id`" is not what the code does.
5. **PVC patch permission.** **HOLDS.**
6. **Task 4.2 verification.** **PARTIAL.**
   - The "refuses" test now calls `serve` (`main.rs:940`), but it can't pass. `err.to_string()` on an `anyhow::Error` prints only the outermost context ("startup check failed in sandbox namespace", `main.rs:313`), so `.contains("missing required permissions…")` is false.
   - Also, with the `resourceNames`-restricted namespaces ClusterRole, a GET on `default` returns 403, which maps to `StartupCheckError::Api`, not `MissingPermissions`.
   - The "starts" test still calls `verify_startup` only.
   - Fix: assert on `format!("{err:#}")` or on the downcast error, and run `serve` in the success test too.
   - *Blocking:* task 4.2 is ticked.
7. **No unwrap in prod paths.** **HOLDS.**
8. **127.0.0.1 fallback.** **HOLDS**, with a smell left.
   - The chart now renders `controlPlaneServiceName`, `gatewayServiceName` and `controlPlaneTlsServerName`.
   - When those keys are unset, `main.rs` still guesses names (`dark-factory-control-plane`, `dark-factory-egress-gateway`, `{instance_name}-control-plane`). Non-blocking; see R3-N6.
9. **Unused dependencies.** **HOLDS** by inspection; machete was not run.
10. **Config documented.** **PARTIAL.** The README (88 lines) covers the lease, resync, RBAC and TLS name. The new `sandbox.*` keys (`controlPlaneServiceName`, `gatewayServiceName`, `controlPlaneTlsServerName`) and the name-guessing fallback have no reference entries. *Non-blocking.*
11. **Defaults in one place.** **CLAIM WRONG.**
    - Defaults live in df-core `config.rs`, again as serde defaults on df-k8s `ProvisioningConfig` (`df-k8s/src/config.rs:23-68`), and in the Helm chart (`values.yaml` plus `| default …` in `configmap.yaml`).
    - df-k8s also keeps its own `default_control_plane_port`/`default_gateway_proxy_port` (`:107-113`), which duplicate df-core's `default_agent_port`/`default_proxy_port`.
    - `ProvisioningConfig` is built only in code, so its `Deserialize` derive and serde defaults are dead.
    - The copies now **disagree**: the chart's `""` means "cluster default", while the Rust defaults are `kata` and `factory-workspace` (see R3-B1).
    - `from_sandbox_config` and `empty_string_is_none` exist. `empty_string_is_none` has no test.
    - *Blocking:* the copies disagree. Target: df-core holds the single set of defaults, with `runtime_class_name` and `storage_class` defaulting to `None` (R3-B1 decision).

### Round 2
- **R2-B1** (`services get`, namespaces ClusterRole, direct namespace GET). **HOLDS.** See R3-N4 for a related gap.
- **R2-B2** (Service and TLS names from `fullname`). **HOLDS** in the chart (`configmap.yaml`, `certs.yaml` dnsNames).
- **R2-B3** (sandbox issuer). ✔ **NOT FIXED, and a regression.** See R3-B2.
- **R2-B4** (watcher backoff). **HOLDS** (`provider.rs:404,416`).
- **R2-B5** (named targetPort). ✔ **CLAIM WRONG.** See R3-B4.
- **R2-B6** (task checkboxes). **NOT FIXED.**
  - 4.1 and 4.2 are ticked although their verification isn't met (items 3 and 6, R3-N8).
  - The 1.1–1.4 spikes were ticked this round with only README prose as evidence.
  - 5.2 rests on an unverified coverage number.
  - *Blocking.*
- **R2-N1** (forwarder). **HOLDS.**
- **R2-N2** (an unresolved target is dropped). ✔ **NOT FIXED.**
  - On a `resolve_cluster_target` error, `DbClusterTargetsProvider::get_targets` (`main.rs:142-148`) logs and drops the target, then returns `Ok`.
  - Reconcile then applies a policy without that target, so a live run loses access on a transient error.
  - Fix: return `Err` so reconcile skips that run and keeps its current policy.
- **R2-N3** (empty string → None). **HOLDS**. The resulting behaviour is accepted (R3-B1 decision). `empty_string_is_none` has no test.
- **R2-N4** (`egress.rs`). **PARTIAL, CLAIM WRONG.**
  - The real query (`egress.rs:21-27`, `egress_allowlist e LEFT JOIN projects p`) is correct against the schema. The SQL quoted in the fix note (`egress_policies`, `project_key`, `protocol`) does not exist.
  - Ports are not checked for 1..=65535: 0 passes `u16::try_from`.
  - The only test asserts an empty result. Nothing seeds instance or project rows, so scope filtering is untested.
  - It still uses runtime `sqlx::query`, so there is no compile-time check; see R3-N9.
- **R2-N5** (events across relists). **NOT FIXED.**
  - A relist emits `Init`/`InitApply`/`InitDone`, never `Delete`.
  - Nothing compares `started_runs` with the store on `InitDone`, so a pod that vanished during a disconnect never yields Gone (`provider.rs:434-468`).
- **R2-N6** (`events()` drops Lagged). **NOT FIXED.** `provider.rs:~627` still does `filter_map(res.ok())` with no `warn!`.
- **R2-N7** (timeouts). **PARTIAL.**
  - The listed calls are wrapped, including SSAR and `release_lease`, which now clears holder and `renewTime` (`reconcile.rs:147-148`).
  - `ensure_task_volume`, `provision` and `teardown` still have no timeout.
- **R2-N8** (reconcile and lease tests). **PARTIAL.** See R3-B6. The lease test doesn't assert that `renewTime` is cleared in the PUT body.
- **R2-N9** (coverage). **Unverified.**
- **R2-N10** (event plumbing moved out of `main.rs`). **HOLDS.** `K8sSandboxProvider::start` returns a future that `main.rs` spawns into the JoinSet. That is owned, though the fix note's "owns its tasks" overstates it.
- **R2-N11, N12, N13, N14, N15.** **HOLDS.**

### Smells carried over from round 2
- **`serve(…, Option<Client>)`.** **NOT FIXED.** `None` is a test-only branch, and `run_serve_command` still does `match Client::try_default {Ok => Some, Err => return}`.
- **Duplicated run-id label extraction.** **NOT FIXED** (`provider.rs:266,272-277`, `reconcile.rs:221-237`). `run_id_from_metadata` already exists; use it everywhere.
- **`check_permissions` helper.** **HOLDS.**

---

## New issues (round 3)

### Blocking

- **R3-B1 No Kata runtime class and no `factory-workspace` StorageClass by default. RESOLVED BY DECISION (2026-10-21).**
  - The owner accepted the behaviour: many clusters don't have Kata installed, so an unset or empty `runtimeClassName` falls back to the cluster's default runtime. The same applies to `storageClass`, which falls back to the cluster's default StorageClass.
  - The spec was updated to match:
    - `specs/sandbox-provisioning/spec.md`: the "Task volume" and "Sandbox per run" requirements; the "Sandbox spec" scenario is now conditioned on `kata` being configured; new scenario "No runtime class configured".
    - `proposal.md`: "What Changes".
  - **Still open (non-blocking, part of round-1 item 11):** the Rust defaults (`default_runtime_class_name()` = `kata`, StorageClass `factory-workspace`, in df-core and df-k8s) and the README table (`crates/df-k8s/README.md:38,40`) still say `kata` and `factory-workspace`. Make the Rust defaults `None` so a config without the keys behaves like the chart, and update the README.
  - The deployment docs should say that operators who want microVM isolation set `sandboxNamespace.runtimeClassName: kata`.
- **R3-B2 ✔ The CA ClusterIssuer can't find its Secret, so no certificate is issued at all.**
  - `certs.yaml:32-39` turns `{fullname}-ca-issuer` into a **ClusterIssuer** with `ca.secretName: {fullname}-ca-key-pair`.
  - A CA ClusterIssuer reads its Secret from cert-manager's *cluster resource namespace* (default `cert-manager`), but the CA Certificate writes it to `.Release.Namespace` (`certs.yaml:14-22`). The ClusterIssuer never becomes Ready.
  - `_helpers.tpl:111-119` now also points the **control-plane and gateway** Certificates at this ClusterIssuer; they used a working namespaced Issuer before. So the round-2 fix turned "sandbox certs fail" into "all certs fail".
  - The default `existingIssuer.kind` changed to `ClusterIssuer` while `deploy/helm/dark-factory/README.md:91` still documents `"Issuer"`.
  - The cluster-scoped name `{fullname}-ca-issuer` collides between releases that share a fullname in different namespaces.
  - Fix:
    - Keep the namespaced Issuer for the control plane and gateway.
    - For sandboxes, either issue the CA into the cert-manager cluster resource namespace (configurable), or use an `existingIssuer` ClusterIssuer.
    - Document the requirement.
- **R3-B3 ✔ The Helm unit tests fail.** `deploy/helm/dark-factory/tests/chart_test.yaml:282-311` still asserts `isKind: Issuer` for document 2 and `issuerRef.kind: Issuer` for the control-plane Certificate. lefthook's pre-commit runs the Helm unit tests, but the fixer ran only `helm lint`. Update or revert together with R3-B2.
- **R3-B4 ✔ Named targetPort is still wrong.**
  - `provider.rs:154-161` looks for an EndpointSlice port whose `name` equals the **targetPort name**. Kubernetes names EndpointSlice ports after the **Service port's** `name`.
  - So a Service port `http` with `targetPort: web` never matches, and the code silently falls back to the Service port number, which is the original bug.
  - The unit test passes only because it uses the same name for both (`grpc`).
  - Fix: match `ep_port.name == p.name`. Return an error (and skip the run, R2-N2) instead of falling back to `port_num`. Fix the test fixture.
- **R3-B5 ✔ Image-pull failures never emit Gone.**
  - `provider.rs:447-451` emits Gone from Apply only if `started_runs.remove(&run_id)` returns true.
  - A pod in `ErrImagePull`/`ImagePullBackOff`, or `Failed` before it ever reached Running, was never in `started_runs`, so its Gone is suppressed. After a control-plane restart the set is empty, so no Apply-based Gone fires for any run.
  - Spec (sandbox-provisioning): "WHEN the project image can't be pulled THEN a `Gone` event with a reason naming the image pull failure is emitted". Design D4 says the same.
  - Fix: emit Gone unconditionally and dedupe with a separate `gone` set. Add a test for the `start()` loop, not only for `map_pod_event`.
- **R3-B6 The spin regression test doesn't fail without the fix.**
  - `test_reconcile_loop_spin_regression_and_shutdown` (`reconcile.rs:723-785`) advances paused time and sends shutdown, but it asserts nothing about the number of reconcile calls.
  - Without the guard the loop still yields (tokio coop budget), so the test passes either way.
  - AGENTS.md: "When a bug is fixed, add a test that fails without the fix."
  - Fix: count `get_targets` calls while `is_leader=true` after the notify channel closes, and assert they stay bounded.

### Non-blocking

- **R3-N1 ✔ The same spin class remains on the watch channels.** `shutdown.changed()` and `is_leader.changed()` (`reconcile.rs:333,399,404`) return `Err` immediately once their sender is dropped, and the code ignores the `Err`. If the leader task exits while `is_leader` is still `true`, every iteration triggers a reconcile. Fix: `break` on `Err`.
- **R3-N2 Wrong context message.** `main.rs:313` uses `.map_err(|e| anyhow!("{e}")).context("startup check failed in sandbox namespace")` for instance-namespace failures too, and the `anyhow!` wrapping loses the source. Fix: `.context("startup check failed")`. This also unblocks the 4.2 test assertion.
- **R3-N3 Timeout constants.**
  - The 5 s literal is repeated in `startup.rs`, `provider.rs:72,118` and `main.rs`.
  - `LEASE_API_TIMEOUT` is reused for reconcile (`reconcile.rs:188,287`), which is a misleading name.
  - Fix: one `K8S_API_TIMEOUT` const.
- **R3-N4 Startup check misses the slice-watch permission.** `INSTANCE_PERMISSIONS` lacks `endpointslices list/watch` (needed by `slice_watch_task`), and nothing checks permissions in the target namespaces.
- **R3-N5 File structure order.** `startup.rs:168`: the private `check_permissions` comes before the public `verify_startup` (AGENTS.md "File structure").
- **R3-N6 Name-guessing fallbacks.** When the chart keys are unset, `main.rs` falls back to `dark-factory-*` Service names and `{instance_name}-control-plane`. Either make the keys required, or document the fallback.
- **R3-N7 Unchecked cast.** `run_leader_election` casts `duration_seconds as u64` (`reconcile.rs:327`), so a negative value wraps. Fix: `u64::try_from` and validation.
- **R3-N8 Task 4.1's test doesn't verify the task.** `test_reconciliation_live_cluster` calls `reconcile_active_runs` directly. It makes no manual edit, checks no revert, and doesn't exercise the loop or the 30 s window.
- **R3-N9 Test configs and query checking.**
  - The test configs in `render.rs:674` and the `reconcile.rs` live test still use `issuer_kind: "Issuer"`, while the default is now `ClusterIssuer`.
  - `egress.rs` uses runtime `sqlx::query`, while AGENTS.md lists "compile-time checked queries". Fix: `query_as!` with checked-in `.sqlx` metadata.

---

## Looks correct (re-confirmed)
- The `egress.rs` query is correct against the schema; reconcile reads the right Sandbox labels.
- Reserved-label precedence, owner-UID check, IPv6 `/128`, teardown selector.
- hostAliases and NO_PROXY populated from DB targets; control plane at `<ClusterIP>:9443`.
- Watcher backoff; the forwarder owned and lag-tolerant; `release_lease` clears holder and `renewTime`.
- RBAC: `services get`, `namespaces get` restricted with `resourceNames`, PVC `patch`, leases.
- The chart renders Service names and TLS server name from `fullname`.
- No `unwrap`/`expect`/`panic` in production code; `StaticClusterTargetsProvider` is test-only; the `crd.rs` header is fixed.

---

## Round 4 fixes

Status: FIXED / PARTIAL / NOT DONE / DEFERRED. Line numbers refer to the tree after round 4. Paths are relative to the repo root; `k8s` = `crates/df-k8s/src`.

### Earlier findings

| Item | Status | What changed | Test |
|---|---|---|---|
| 1 / R3-B6 / R3-N1 spin | FIXED | `reconcile.rs` `run_policy_reconcile_loop` (~401): `notify_active` guard kept; the loop now `break`s when `shutdown.changed()` or `is_leader.changed()` returns `Err`. `run_leader_election` (~305) treats an `Err` from `shutdown.changed()` as shutdown (steps down, releases the lease) and returns `Result`. | `closed_notification_channel_does_not_spin_the_reconcile_loop` (~899) counts polls of the loop future. **Verified to fail without the guard**: with `, if notify_active` removed it failed with "reconcile loop is spinning: 103 polls"; restored, it passes. It also asserts 1..=4 `get_targets` calls. Also `reconcile_loop_stops_when_the_leader_channel_sender_is_gone`, `..._shutdown_sender_is_gone`, `leader_election_ends_when_the_shutdown_sender_is_gone`. The two "sender is gone" loop tests were not checked to fail without their fix (a failing run would hang on paused time). |
| 3 / D5 / R3-N4 watches | FIXED | New `watch_cluster_target_changes` (`reconcile.rs` ~367): watches `Service` and `EndpointSlice` in each `clusterTargets.namespaces` entry (cluster-wide with `anyNamespace`), `.default_backoff()`, logs watch errors, `try_send`s a reconcile trigger on Apply/Delete/InitDone. No targets: the future stays pending (an ended future would look like a failed task). `main.rs` no longer watches the instance namespace. New `clusterTargets` config section (`df-core` `ClusterTargetsConfig`, rendered by the chart). Startup check: new `TARGET_PERMISSIONS` (services get/list/watch, endpointslices list/watch) checked per target namespace or cluster-wide (`startup.rs` ~160, ~204). RBAC: `rbac.yaml` already grants services and endpointslices get/list/watch per target namespace (Role) or cluster-wide (ClusterRole); the endpointslices rule on the instance Role is removed (nothing watches the instance namespace any more). | `forward_changes_notifies_on_changes_and_survives_errors`, `forward_changes_does_not_block_on_a_full_channel`, `watching_no_namespaces_never_completes`, `startup_checks_watch_permissions_in_every_target_namespace`, `startup_checks_cluster_wide_permissions_with_any_namespace`, `startup_succeeds_when_everything_is_allowed`; chart tests for the Roles/ClusterRole. The watchers themselves (`change_stream`) are not tested against a server. |
| 4 label claim | HOLDS (no code change) | Reconcile now reads the run id through `run_id_from_metadata` (see dedupe item). | existing |
| 6 / R3-N2 task 4.2 | PARTIAL | `main.rs` ~303: `.context("startup check failed")` without `anyhow!("{e}")`. The 4.2 tests (~951, ~992) assert on the downcast `StartupCheckError::MissingPermissions` and `format!("{err:#}")`; the success test now runs `serve` with the chart's RBAC and checks the health port. A `403` on the sandbox namespace lookup is now reported as `MissingPermissions` (`startup.rs` ~221) instead of `Api`, because the chart's ClusterRole only grants `get` on that one namespace. The tests stay `#[ignore]` (dev cluster + PostgreSQL container) and were **not run**. | `forbidden_namespace_lookup_is_a_missing_permission` (runs); the two `serve_*` tests compile but did not run. |
| 8 / R3-N6 name fallbacks | FIXED (documented) | Fallback chain kept; documented per key in `crates/df-k8s/README.md` and in the doc comments of `SandboxSectionConfig`. The chart always sets the three keys. | none (documentation) |
| 10 README | FIXED | `crates/df-k8s/README.md`: reference table with required/default/effect for every `sandbox.*` and `clusterTargets.*` key, fallbacks, behavior reference (target resolution failure, named targetPort, events, timeouts, loops), RBAC incl. target namespaces. Chart README: new `sandboxNamespace.*` and `tls.ca.*` rows, "TLS issuer" and "Cluster targets" sections. | n/a |
| 11 / R3-B1 follow-up defaults | FIXED | One place: `impl Default for SandboxSectionConfig` (`df-core/src/config.rs` ~246); struct-level `#[serde(default)]`; the per-field `default_*` functions and their re-exports from `df-k8s` are gone. `storage_class` and `runtime_class_name` default to `None`. `ProvisioningConfig` lost `Serialize`/`Deserialize`, the serde defaults and `default_control_plane_port`/`default_gateway_proxy_port`. README table updated. The chart's `values.yaml` still repeats the values (it is the install interface); the chart always renders every key. | `sandbox_section_defaults_leave_runtime_class_and_storage_class_unset`, `sandbox_section_empty_strings_mean_unset` (covers `empty_string_is_none`), `cluster_targets_default_to_no_namespaces` (df-core); `from_sandbox_config_copies_section_values` (df-k8s). |

### Round 2

| Item | Status | What changed | Test |
|---|---|---|---|
| R2-B3 / R3-B2 issuer | FIXED, design decision below | see "R3-B2" | `helm unittest`, `bazel test //deploy:helm_chart_test` |
| R2-B5 / R3-B4 named targetPort | FIXED | `provider.rs` `resolve_cluster_target` (~78): matches the EndpointSlice port whose `name` equals the Service port's `name`; unresolved is `ProviderError::Backend` (no fallback to the Service port number). | `named_target_port_is_resolved_via_the_service_port_name` (fixture now has port `grpc` / targetPort `grpc-internal`; the old code would have returned 50051, not 8080), `unresolvable_named_target_port_is_an_error_not_a_guess`. I did not run the old code against the new fixture. |
| R2-B6 tasks | FIXED | `tasks.md`: 1.1-1.4, 3.1, 4.1, 4.2, 5.1 unticked and marked "pending dev-cluster verification". 5.2 stays ticked: `cargo llvm-cov -p df-k8s` ran and reported 86.88 % lines (recorded in the README). README spike section retitled to "assumptions, not verified". | n/a |
| R2-N2 target failure | FIXED | `main.rs` `DbClusterTargetsProvider::get_targets` (~110) returns `Err` if any Service cannot be resolved (logic in `provider.rs` `resolve_cluster_targets` ~201). Reconcile already skips a run on `Err`; provisioning fails. Documented on the trait and in the README. | `resolving_targets_fails_if_one_service_cannot_be_resolved`. The DB part of `get_targets` is not tested. |
| R2-N4 / R3-N9 egress | PARTIAL | Ports outside 1..=65535 (including 0) fail the listing (`port_from_db`, `egress.rs` ~59). Seeded DB tests added (`rules_are_filtered_by_scope_and_project`, `out_of_range_stored_port_fails_the_listing`: instance and two project scopes, an `http-host` row, ordering, `None` project key). **Still a runtime `sqlx::query`**: `.sqlx` metadata can only be generated with `cargo sqlx prepare` against a live PostgreSQL; this sandbox has no PostgreSQL, no `sqlx-cli` and no container socket. Documented in the doc comment. | `port_from_db_accepts_only_valid_ports` runs. The two DB tests need a container and **did not run**; their SQL is unverified. |
| R2-N5 / R3-B5 events | FIXED | `provider.rs`: `EventTracker` (~613) with separate `started`/`gone` sets; Gone is emitted once per run for failed pods, image-pull/config errors (also if never Started), pod deletes and sandbox deletes. On `InitDone` it emits Gone for runs in `started` that the relist did not return. The loop is `forward_events` (~683), driven by `K8sSandboxProvider::start`. | `event_loop_emits_gone_for_image_pull_failure_without_a_prior_started`, `event_loop_emits_gone_for_failed_pod_and_started_once`, `event_loop_emits_gone_for_pods_that_vanished_during_a_relist` (these feed `forward_events`), `tracker_emits_one_gone_when_the_sandbox_is_deleted`. |
| R2-N6 Lagged | FIXED | `events()` (~581) logs `warn!(skipped, ...)` on `Lagged`. | `events_stream_survives_lag_and_keeps_later_events` (stream survives; the warning itself is not asserted). |
| R2-N7 / R3-N3 timeouts | FIXED | One `df_k8s::K8S_API_TIMEOUT` (5 s, `lib.rs`), used in `startup.rs`, `provider.rs`, `reconcile.rs`, `main.rs`. `ensure_task_volume`, `provision` and `teardown` go through `api_call` (~599) or an explicit timeout (delete). `LEASE_API_TIMEOUT` removed. | `ensure_task_volume_times_out_when_the_api_does_not_answer` (paused time). `provision`/`teardown` timeouts use the same helper but have no separate test. |
| R2-N8 lease test | FIXED | `test_lease_acquire_and_release` now seeds a `renewTime` and asserts the PUT body has neither `holderIdentity` nor `renewTime`. Not checked against the old code. | `test_lease_acquire_and_release` |

### Smells carried over

| Item | Status | What changed |
|---|---|---|
| `serve(…, Option<Client>)` | FIXED | `serve` takes a `SandboxBackend` (`main.rs`): `KubernetesBackend { client }` in production; tests use a test-only `UnconfiguredBackend`. `run_serve_command` no longer matches on the client. The placeholder provider and secret store moved into a `#[cfg(test)]` module in `unconfigured.rs`. The `serve` tests that use `UnconfiguredBackend` need a PostgreSQL container and **did not run**. |
| Duplicated run-id extraction | FIXED | `reconcile.rs` (~214), `resolve_identity_from_stores` and `map_pod_event` use `run_id_from_metadata`. |

### New issues (round 3)

| Item | Status | Notes |
|---|---|---|
| R3-B1 follow-up | FIXED | See item 11. |
| R3-B2 | FIXED, see design | |
| R3-B3 | FIXED | `chart_test.yaml` updated; it also had two failures from earlier rounds (RBAC document count 10 vs 12, cilium verb `update` that the template no longer grants). `helm unittest`: 15 passed. |
| R3-N5 file order | FIXED | `startup.rs`: public `verify_startup` before the private `check_permissions`; `reconcile.rs`, `provider.rs`: private helpers after public items. |
| R3-N7 cast | FIXED | `u64::try_from` plus a positive check in `run_leader_election` (error instead of wrap); `i64::from` for the lease duration. Test: `leader_election_rejects_non_positive_lease_duration`. |
| R3-N8 task 4.1 live test | DEFERRED | `test_reconciliation_live_cluster` still calls `reconcile_active_runs` directly; it needs the dev cluster to be rewritten and verified. Task 4.1 is unticked. |
| R3-N9 test configs | FIXED | `issuer_kind: "ClusterIssuer"` in `render.rs`, `provider.rs`, `reconcile.rs` test configs; two render snapshots updated (`issuer-kind: ClusterIssuer`). Query checking: see R2-N4. |

### R3-B2: design decision

- **Problem:** a CA `ClusterIssuer` reads `spec.ca.secretName` from cert-manager's cluster resource namespace, not from the release namespace.
- **Chosen design:** one CA for everything. With `tls.ca.create: true` the chart creates the self-signed bootstrap `Issuer` and the CA `Certificate` in `tls.ca.clusterResourceNamespace` (default `cert-manager`) and a cluster-scoped `ClusterIssuer`. Control plane, gateway **and** sandbox certificates are issued from that ClusterIssuer. Names are `<release-namespace>-<fullname>-…` (base truncated to 40 characters), so two releases with the same name in different namespaces do not collide.
- **Deviation from the suggestion:** I did **not** keep a namespaced `Issuer` for the control plane and gateway. The control plane trusts sandbox client certificates through the `ca.crt` of its own certificate, and a namespaced Issuer in the release namespace would have its own CA key pair, different from the ClusterIssuer's. Sandbox client certificates would then fail verification. A single CA avoids copying a Secret between namespaces.
- **Consequences:** installing the chart needs permission to create cert-manager objects in the cluster resource namespace. If that is not possible, use `tls.ca.create: false` with `tls.ca.existingIssuer` (default kind is now `ClusterIssuer`). The sandbox namespace still contains no Secret (the CSI driver writes into a tmpfs volume).
- **Defaults in sync:** `sandboxNamespace.issuerName`/`issuerKind` default to `""`; the chart then renders the same name and kind via the `dark-factory.issuer.name`/`.kind` helpers that the Certificates use (test: `points sandbox certificates at the same issuer the chart renders`). The rendered config was parsed with `InstanceConfig::from_yaml_str` in a throw-away test (removed). Chart README line 91 default fixed. Not tested on a real cluster.

### Round 4 verification

All commands run from the repo root, sandbox disabled, toolchain 1.99.1 (`rustup show active-toolchain` OK).

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass, no warnings |
| `cargo test -p df-k8s` | pass: 36 passed, 0 failed, 2 ignored (dev-cluster tests) |
| `cargo test -p df-core` | pass: 19 passed |
| `cargo test -p dark-factory` | 7 passed, 2 ignored, **2 failed**: `serve_runs_the_agent_port_next_to_the_ui_port` and `serve_requires_agent_tls_configuration` need a PostgreSQL container (`/var/run/docker.sock` not found). Not run. |
| `cargo test -p df-db --lib` | 4 passed, **13 failed**: all need a PostgreSQL container. That includes the two new egress DB tests. `port_from_db_accepts_only_valid_ports` passes. |
| `cargo machete` | pass, no unused dependencies |
| `cargo deny check` | pass (advisories, bans, licenses, sources ok; warning about unmatched license allowance `Unicode-DFS-2016`) |
| `helm lint deploy/helm/dark-factory` | pass (1 chart linted, 0 failed; "icon is recommended") |
| `helm-unittest deploy/helm/dark-factory` (via `build/with_tools.sh`) | pass: 15 tests |
| `bazel --bazelrc=.bazelrc.agents build //crates/...` | pass |
| `bazel --bazelrc=.bazelrc.agents test //crates/df-k8s/... //crates/df-core/...` | pass, 2 of 2 test targets |
| `bazel --bazelrc=.bazelrc.agents test //deploy:helm_chart_test //deploy:helm_chart_unittest` | pass (kubeconform / kube-linter and unit tests) |
| `cargo llvm-cov -p df-k8s` | 86.88 % lines (3,141 lines, 412 missed), 85.81 % regions; includes inline test code |
| typos, buildifier (lefthook) | pass |

Not run: dev-cluster tests (tasks 1.x, 3.1, 4.1, 4.2, 5.1, the `serve_*` RBAC tests, `test_provider_live_cluster_integration`); all container-backed PostgreSQL tests (no container socket in the sandbox); `cargo sqlx prepare` (no PostgreSQL, no `sqlx-cli`). No dependency was added, so `Cargo.lock` and the Bazel lock files were not touched by this round.


---

## Round 4, follow-up (Podman, CA note, owner verification)

### Changes

- **R3-N9 / R2-N4 egress query: FIXED.** `crates/df-db/src/egress.rs` `list_cluster_services` now uses `sqlx::query!` (columns `namespace!`/`service!` overridden because the schema allows NULL). Metadata generated with `sqlx-cli 0.9.0` (installed with `cargo install`, not a workspace dependency) against PostgreSQL 17 started with Podman and the repo migrations applied: new file `.sqlx/query-3aad8b60d9cc870ef26b51f8912109961663f8062c49a563796985cd9aee0ee9.json`. `cargo sqlx prepare --workspace` also rewrote the 9 existing `.sqlx` files (this CLI version adds `origin` info); I reverted those, so only the new file is added. `SQLX_OFFLINE=true cargo build --workspace --all-targets` passes without `DATABASE_URL`.
- **Container-backed tests now ran** (Podman socket, `DOCKER_HOST=unix://$XDG_RUNTIME_DIR/podman/podman.sock`, `TESTCONTAINERS_RYUK_DISABLED=true`). This includes the two seeded egress tests (`rules_are_filtered_by_scope_and_project`, `out_of_range_stored_port_fails_the_listing`) and the `dark-factory` `serve_*` tests that use `UnconfiguredBackend`. They passed on the first run; no code fix was needed. This supersedes "did not run" / "SQL unverified" for those tests in the tables above.
- **R3-B2 location: unchanged, by decision.** The CA stays in `tls.ca.clusterResourceNamespace`. Added the reason to `deploy/helm/dark-factory/README.md` and to the `values.yaml` comment: a CA ClusterIssuer only reads its Secret from cert-manager's cluster resource namespace; sandboxes need a ClusterIssuer because their CSI CertificateRequests are created in the secret-free sandbox namespace; both must use the same CA. Setting the value to the release namespace only works if cert-manager runs with `--cluster-resource-namespace=<release namespace>`.
- **Dev-cluster tasks re-ticked: 1.1-1.4, 3.1, 4.1, 4.2, 5.1.** Each carries "(verified on the dev cluster by the project owner, per their statement on 2026-10-21)". **This rests solely on the owner's statement; I did not run or see any dev-cluster command.** The `crates/df-k8s/README.md` spike section now says the same. The README spike table content is the design's expected behavior, not independently recorded results.
- **R3-N8 stays open / deferred.** The `test_reconciliation_live_cluster` code still calls `reconcile_active_runs` directly; it does not check that a manual edit is reverted or the 30 s window. Task 4.1 is ticked on the owner's statement, not on this test.
- The two `#[ignore]` `serve_*` RBAC tests and the live provider/reconcile tests were still not run by me.

### Round 4 verification (re-run)

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass |
| `cargo test --workspace` (Podman) | pass, 0 failures. dark-factory 9 passed / 2 ignored; df-agent-channel 69 + 24; df-core 19; df-db 17; df-k8s 36 passed / 2 ignored; df-proto 2; df-test-postgres 6; df-web 3; factory-agent 90 passed / 1 ignored; fake-acp-agent 8; forgejo-plugin 1 |
| `SQLX_OFFLINE=true cargo build --workspace --all-targets` (no `DATABASE_URL`) | pass |
| `cargo machete` | pass |
| `cargo deny check` | pass (advisories, bans, licenses, sources ok) |
| `helm lint` | pass |
| `helm-unittest` | 15 passed |
| typos | pass |
| `bazel --bazelrc=.bazelrc.agents build //crates/...` | pass |
| `bazel --bazelrc=.bazelrc.agents test //crates/df-k8s/... //crates/df-core/...` | pass, 2 of 2 |
| `bazel --bazelrc=.bazelrc.agents test //... --test_env=DOCKER_HOST=unix://$XDG_RUNTIME_DIR/podman/podman.sock --test_env=TESTCONTAINERS_RYUK_DISABLED=true` | pass, 16 of 16 test targets. I did not inspect per-test counts inside the Bazel logs, so I cannot confirm from here that the container tests really reached Podman under the Bazel sandbox (some results may come from the remote cache). |
| `cargo llvm-cov -p df-k8s` | 86.88 % lines (unchanged) |

`Cargo.lock` and `MODULE.bazel.lock` were already modified before this round and were not changed by it (no dependency added; `sqlx-cli` is installed outside the workspace). The temporary Podman PostgreSQL container used for `sqlx prepare` was removed; the shared `df-test-postgres` test container remains, as designed.

---

## Dev-cluster confirmation (owner, 2026-10-21)

- The owner ran `bazel run //deploy:dev`. Everything came up, including the certificates from the new CA ClusterIssuer and the RBAC changes.
- The owner then ran `cargo test -p dark-factory -- --ignored serve_ --nocapture` against the dev cluster, through Podman: `serve_startup_check_refuses_to_start_with_missing_role` passed and `serve_starts_with_the_charts_rbac` passed (2 passed, 0 failed).
- Task 4.2's verification is therefore confirmed by this run, not only by the owner's statement.

---

# Round 5 review (2026-10-21)

## What was reviewed
- **Change set:** `git diff HEAD` plus the untracked `crates/df-k8s/`, `crates/df-db/src/egress.rs` and `.sqlx/query-3aad8b60….json`.
- **HEAD moved while the review ran:** the branch was rebased onto `origin/main`, from `cc51891` to `9c9dd30` (four upstream commits). The rebase's autostash didn't apply cleanly; see R5-B1. `stash@{0}` (autostash) still exists.
- **Owner decisions taken as fixed:**
  - Runtime class and StorageClass are optional.
  - The CA lives in cert-manager's cluster resource namespace, with a ClusterIssuer.
  - The dev-cluster tasks are ticked on the owner's statement.
  - Task 4.2 is confirmed by the owner's dev-cluster run.

## Commands run by the reviewers

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass |
| `cargo test -p df-k8s -p df-core` | df-core 19 passed; df-k8s 36 passed, 2 ignored (dev cluster) |
| `cargo test -p df-db -p dark-factory` (Podman) | df-db 17 passed (includes both seeded egress tests); dark-factory 9 passed, 2 ignored |
| `cargo machete` | pass |
| `helm lint` | pass; it does not catch the duplicate key in R5-B1 |
| helm-unittest | 15/15 passed |
| SHA-256 of the egress query string | matches the `.sqlx` file name and `hash` |

Not run in round 5: Bazel, `cargo deny`, the dev-cluster `#[ignore]` tests.

## Round-4 fix validation

| Item | Verdict | Evidence / gap |
|---|---|---|
| Spin guard and regression test (1, R3-B6) | HOLDS | Without the guard the test fails with ~103 polls (`reconcile.rs:438`). |
| `changed()` Err → break (R3-N1) | HOLDS | `reconcile.rs:418,423,329`. Both "sender is gone" tests **hang** instead of failing without the fix (R5-N12). |
| D5 watches (item 3) | HOLDS | Services and EndpointSlices are watched per `clusterTargets.namespaces`, or with `Api::all` when `anyNamespace` (`reconcile.rs:367-397,490-520`). RBAC and `TARGET_PERMISSIONS` match. No test runs the watchers against a server. |
| R2-N2: skip the run, keep its policy | HOLDS | `reconcile.rs:242-251`; provisioning fails instead (`provider.rs:478-484`); documented. New consequence: R5-B2. |
| R3-B4 named targetPort | HOLDS | Matched by the Service port name, error on miss (`provider.rs:139-155`); tests at `:1086,:1102`. Fallback for unnamed ports: R5-N4. |
| R3-B5 / R2-N5 events | PARTIAL | Gone for never-started pods and for Failed, deduped, Lagged warned. `InitDone` only covers runs in `started`: a never-Running pod that vanished during a disconnect, and every vanished pod after a control-plane restart, get no Gone. The Sandbox watcher's `InitDone` is ignored (`provider.rs:651-672`). See R5-N10. |
| R3-B2 cert chain | HOLDS | Control plane, gateway and sandboxes all use the `<ns>-<fullname>-ca-issuer` ClusterIssuer; the client CA comes from the same CA; no Secret in the sandbox namespace. Owner confirmed it works on the dev cluster. Gap: R5-N8 (schema still allows `Issuer`). |
| Runtime class / StorageClass `None` | PARTIAL | Rendered as absent fields (`render.rs:87,379`). No render test or snapshot covers the spec scenario "No runtime class configured", and no PVC test covers `storage_class: None` (R5-N6). |
| Startup check: 403 → MissingPermissions | HOLDS | `startup.rs:221-227`, test at `:566`. |
| Startup check: "each missing item" | PARTIAL | It returns at the first namespace with gaps (`startup.rs:243-250`), and `secrets get` is not checked (R5-N7). |
| Single source of defaults (11) | PARTIAL | Rust defaults live only in df-core `impl Default`. The chart still repeats `issuerGroup` as `| default "cert-manager.io"` (`configmap.yaml:40,114`), and the runtime image tag `0.1.0` is hard-coded in both df-core and the chart. |
| `SandboxBackend` replaces `serve(Option<Client>)` | HOLDS | A trait with one production impl and one test impl, which "traits over closures" allows. |
| Run-id extraction deduplicated | HOLDS | Only `run_id_from_metadata` (`provider.rs:349`) reads the label. |
| `K8S_API_TIMEOUT` | PARTIAL | Covers the provider, teardown and reconcile calls. `K8sSecretStore::get` (`secrets.rs:33`) has no timeout (R5-N9). |
| File structure order | HOLDS | One new violation, R5-N14. |
| `u64::try_from` lease duration | HOLDS | `reconcile.rs:314-321`, test at `:1043`. |
| Egress `query!` and `.sqlx` | HOLDS | The SQL is byte-identical to the metadata. The `namespace!`/`service!` overrides are safe because of the schema's CHECK. |
| R3-N2 context message | HOLDS | — |
| Task 4.1 vs R3-N8 | INCONSISTENT | 4.1 is ticked on the owner's statement, but the integration test the task names ("manual policy edit is reverted and a new target appears within 30 s") doesn't exist; `test_reconciliation_live_cluster` calls `reconcile_active_runs` directly. The README spike text is expected behaviour, not recorded results. Recorded here as accepted by the owner. |

## Blocking

- **R5-B1 ✔ `deploy/helm/dark-factory/values.yaml` is still unmerged.**
  - `git ls-files -u` shows stages 1/2/3. This comes from the rebase autostash, not from the code changes.
  - Upstream `9c9dd30` switched the file to single quotes, so it now has `runtimeClassName: ''` (line 56) and `runtimeClassName: ""` (line 58).
  - Helm silently keeps the last key, so lint and unittest pass. Strict YAML parsers reject the duplicate key, and the commit can't be made while the file is unmerged.
  - Fix: delete line 56 (keep the commented `""` entry, or use `''` to match the upstream style), check the rest of the file against upstream, `git add` it, then drop `stash@{0}` once it is confirmed applied.
- **R5-B2 One broken allowlist entry freezes every run's policy, including revocations.** *Spec reviewer rated this non-blocking; upgraded because it is a security and spec issue.*
  - `resolve_cluster_targets` (`provider.rs:201-220`) is all-or-nothing, and reconcile keeps the old policy on any error (the R2-N2 fix).
  - One instance-scoped entry that fails to resolve is enough: a Service outside `clusterTargets` (403), a missing Service (404), or a named targetPort with zero ready endpoints. After that, **no** run's policy is re-rendered, so a *removed* allowlist entry stays allowed until the broken one is fixed. The same entry also fails provisioning of every run.
  - Nothing checks allowlist namespaces against `clusterTargets`, so this is easy to hit.
  - Spec (run-network-policy): "When cluster-service entries change … SHALL re-render and overwrite the policies of all active runs within 30 seconds."
  - Fix: resolve each target on its own and keep the last good resolution for a target only while its rule still exists, so removed rules are always dropped. Or check the namespaces against `clusterTargets` when the entry is written. At minimum, log which entry blocks reconciliation.

## Non-blocking

### Spec and behaviour
- **R5-N1 Headless check bypassed in reconcile.** Spec: "Headless Services SHALL be rejected unless the entry sets `allowDns`." The check exists only in `render_sandbox` (`render.rs:104-111`). `reconcile_active_runs` calls `render_network_policy` directly (`reconcile.rs:253`), so a headless rule without `allowDns` added mid-run reaches live runs' policies, while new provisions fail. Fix: move the check into target resolution.
- **R5-N2 Empty `target_ports` renders `ports: None`, which allows all ports** (`render.rs:566,587`). It happens for a headless Service without ports. Security-relevant. Fix: reject the target (error) instead of rendering a portless rule.
- **R5-N3 Only the first EndpointSlice port is used** for a named targetPort (`provider.rs:146-150`). Pods that use different numbers for the same port name end up in separate slices, and those pods are blocked. Fix: collect all matching ports.
- **R5-N4 Port guessing for unmatched configured ports.** When no Service port matches the configured ports, `provider.rs:161-163` uses the configured ports as target ports, the same kind of guess R3-B4 removed. Fix: return `ProviderError::Backend`.
- **R5-N5 Runtime-image pull failures are never reported.**
  - `map_pod_event` only looks at `container_statuses` (`provider.rs:313`).
  - With `runtimeDelivery: initContainer`, the failure shows up in `init_container_statuses`. With image volumes, the pod stays in ContainerCreating.
  - Either way no Gone is emitted and the run hangs until the connect timeout. Fix: include init-container statuses.
- **R5-N6 No test for "No runtime class configured"** (the new spec scenario): every render config uses `Some("kata")` (`render.rs:672`). There is also no PVC test with `storage_class: None`. Fix: add snapshots for both.
- **R5-N7 Startup check is incomplete.** It returns at the first namespace with gaps (`startup.rs:243-250`), so later namespaces aren't named, contrary to "a message naming each missing item". `secrets get` in the instance namespace (needed by the SecretStore) isn't checked. Fix: collect all gaps, and add `secrets get`.
- **R5-N8 Schema allows a broken issuer kind.** `values.schema.json:85` still allows a sandbox `issuerKind: Issuer`, which renders a reference to an Issuer that doesn't exist in the sandbox namespace. Fix: restrict the enum or document the constraint.
- **R5-N9 Timeouts and debounce.**
  - `K8sSecretStore::get` (`secrets.rs:33`) has no `K8S_API_TIMEOUT`.
  - With `anyNamespace`, every EndpointSlice change in the cluster triggers a full reconcile (`reconcile.rs:508-520`). Fix: debounce, for example a 1–2 s coalescing window.
- **R5-N10 Gone after a disconnect or restart.**
  - The `InitDone` handling covers only runs started in the current process, and the Sandbox `InitDone` is ignored.
  - Events go through `broadcast`, which drops them while there's no subscriber, so the first relist can fire before the scheduler subscribes (`provider.rs:446-450`, `main.rs`).
  - Fix: confirm and document that the scheduler resyncs active runs from the DB at startup (or reconcile against the store at `InitDone` for every run with a Sandbox).

### Standards and smells
- **R5-N11 A default kept in a third place.** `configmap.yaml:40,114` has `| default "cert-manager.io"` although `values.yaml` sets it. Fix: drop the template `default`.
- **R5-N12 Tests that hang.** The two "sender is gone" tests (`reconcile.rs:954,985`) hang instead of failing without their fix. Fix: bound the polls as in the spin test.
- **R5-N13 Duplicated configmap block.** The ~25-line `sandbox:` block appears twice in `configmap.yaml` (~27-54 and ~101-128). Fix: move it into a named template and `include` it twice.
- **R5-N14 Impl far from its type.** `impl SandboxBackend for KubernetesBackend` (`main.rs:287`) is about 120 lines from its struct (`:168`). AGENTS.md says impl blocks stay next to their type.
- **R5-N15 Data Clump.** `ProvisioningConfig` copies 14 fields of `SandboxSectionConfig` one by one (`df-k8s/src/config.rs:54-87`), and `resolve_cluster_ips` takes 7 parameters (`main.rs:429`). Fix: embed the section struct, and pass `&SandboxSectionConfig`.
- **R5-N16 Casts and errors.**
  - `as i32` casts at `render.rs:494,525,543`. Fix: `i32::from`.
  - `provider.rs:483` re-wraps an error that is already a `ProviderError`. Fix: `?`.
  - A hand-written `unwrap_or_else(|| panic!(…))` in a test (`main.rs:975`). Fix: `assert!(matches!(…))`.
- **R5-N17 Weak lag test.** `events_stream_survives_lag_and_keeps_later_events` (`provider.rs:1293`) doesn't check that the event sent after the overflow arrives.
- **R5-N18 `.sqlx` format mismatch.** Only the new file has the `origin` fields of sqlx-cli 0.9.0. It works. Fix: regenerate everything with one CLI version later.
- **R5-N19 Shutdown waits for a reconcile pass.** `trigger_reconcile` runs inside the `select!` arm (`reconcile.rs:429-466`). A long pass can outlast `SHUTDOWN_GRACE` and be aborted; that's safe because the patches are idempotent. Fix: document it, or check `stop` between runs.

## Checked, no findings
- Identity resolution scenarios: namespace, cached UID, IP, Sandbox owner UID plus instance label (`provider.rs:240-287`).
- Credential resolution: the missing-key error names the Secret and key and never the value, and values are re-read on every use.
- NetworkPolicy contents against the run-network-policy spec: no ingress, control plane and gateway on their ports, `/32` and `/128` ipBlocks, kube-dns 53 only with `allowDns`.
- Helm keys match the serde names for all `sandbox.*` and `clusterTargets.*` keys.
- No `unwrap`/`expect`/`panic!` in new production paths, no lock guards across `.await`, cancel-safe `select!` branches, every spawned task owned by the JoinSet, structured tracing without secrets.

## Summary

| | Blocking | Non-blocking |
|---|---|---|
| Round 5 | 2 (R5-B1, R5-B2) | 19 |
| Round-4 fixes | all HOLD or PARTIAL; the PARTIAL gaps are listed above as R5-N items | |

Worst issue per axis:
- **Spec:** R5-B2, where one unresolvable allowlist entry freezes every policy, including revocations.
- **Standards:** R5-B1, the unmerged `values.yaml` with a duplicate key.

---

# Round 6 fixes

Paths: `k8s` = `crates/df-k8s/src`. Line numbers refer to the tree after round 6. Status: FIXED / PARTIAL / NOT DONE.

## Merge check (R5-B1): FIXED

Compared `deploy/helm/dark-factory/values.yaml` with upstream `9c9dd30` and the autostash (`git show stash@{0}:…`, read-only; the stash was not applied or dropped).

- **Duplicate key:** the two `runtimeClassName` lines were two real keys, not a comment (`''` upstream-style and `""` ours). Removed the duplicate; the comment stays above the single key. A strict YAML parse (PyYAML with a duplicate-key check) now succeeds: "no dup".
- **Quoting:** upstream moved to single quotes; my added keys used double quotes. Converted them to single quotes for consistency.
- **Nothing lost:** flattened comparison with upstream: no key missing, the only value change is the intended `tls.ca.existingIssuer.kind: ClusterIssuer` (was `Issuer`); 13 keys added by us. Comparison with the autostash: the only differences are upstream's resource changes (`controlPlane.resources.requests.cpu` 100m to 50m; `limits.cpu` removed for control plane and gateway), which are kept.
- **Semantic overlap with the other upstream commits:** upstream touched `.github/workflows` (path filters, an OpenSpec validation job), `design/`, `product/`, `openspec/changes/*` (other changes) and `values.yaml` only. No `Cargo.toml`, `MODULE.bazel`, chart template or chart test changed upstream. One follow-up: the chart README still documented the old resource defaults; fixed (`controlPlane.resources.requests.cpu` "50m", the two `limits.cpu` rows removed, "no CPU limit by default" noted).
- `helm lint` and `helm-unittest` do not catch duplicate keys; the strict parse above does.

## R3-N8: task-4.1 integration test: FIXED (written, not run)

`k8s/reconcile.rs` `policy_loop_live_reverts_manual_edit_and_adds_new_target` (~725) with `drive_policy_loop` (~818). `#[ignore = "requires dev cluster"]`. It compiles and clippy is clean; **I did not run it and did not touch the cluster.**

- **What it does:** provisions a run (`DF-889`, run 1) with a fake allowlist provider that returns one target, starts the real `run_policy_reconcile_loop` with forced leadership (a `watch` channel, no Lease) as `serve` does, then (1) merge-patches `spec.egress` of the NetworkPolicy to `[]` by hand and waits up to 45 s until the loop restored the original rule count, (2) flips the fake provider to return a second target and waits up to 45 s for the rule count to grow by one. Both rely on the 30 s resync, no notification is sent.
- **Prerequisites:** `KUBECONFIG` pointing at the dev cluster with permission to create/patch/delete in `dark-factory-dev-sandboxes`: PVCs, `agents.x-k8s.io` Sandboxes, NetworkPolicies, pods get/list; the agent-sandbox controller running; namespaces `dark-factory-dev` and `dark-factory-dev-sandboxes` existing; the `alpine:latest` image pullable. The cluster Services named by the fake targets (`nativelink/scheduler`, `forgejo/ssh`) do not need to exist.
- **Command:** `cargo test -p df-k8s --lib policy_loop_live -- --ignored --nocapture`
- **Changes in the cluster:** PVC `task-df-889`, Sandbox `run-df-889-1` (and its pod) and NetworkPolicy `run-df-889-1` in `dark-factory-dev-sandboxes`, plus one manual patch of that NetworkPolicy.
- **Cleanup:** after the scenario, whether it passed or not, it tears down the run (Sandbox), deletes the NetworkPolicy and the PVC (errors ignored). If the process is killed, remove them with `kubectl -n dark-factory-dev-sandboxes delete sandbox/run-df-889-1 networkpolicy/run-df-889-1 pvc/task-df-889`.
- The older `test_reconciliation_live_cluster` (calls `reconcile_active_runs` directly) is unchanged.

## R5-B2 one bad allowlist entry freezes all policies: FIXED

- `k8s/provider.rs` `ClusterTargetResolver` (~230) replaces the all-or-nothing `resolve_cluster_targets`: each rule resolves on its own. On failure it uses the rule's last good resolution (warn naming the rule) or skips the rule (warn naming it, fail closed). Resolution is driven by the rules passed in, so a removed rule is always dropped even if cached. `main.rs` `DbClusterTargetsProvider::get_targets` fails only if the database query fails.
- Behavior: a bad entry no longer fails provisioning of other runs or freezes reconciliation; the affected run starts without that target. A broken instance-scoped entry applies to every run, so every run lacks that target until fixed (visible in warnings). Documented in `crates/df-k8s/README.md` ("Behavior Reference").
- Tests: `one_unresolvable_rule_does_not_block_the_others` (~1551), `last_good_resolution_is_kept_only_while_the_rule_exists` (~1566, also covers removal).

## Round-5 non-blocking items

| Item | Status | What changed | Test |
|---|---|---|---|
| R5-N1 headless without `allowDns` | FIXED | Rejected in `resolve_cluster_target` (so reconcile too); `render_sandbox` keeps its check. | `headless_service_without_allow_dns_is_rejected_at_resolution` (~1640) |
| R5-N2 empty ports allow all | FIXED | Resolver errors on an empty port list; `render_network_policy` also skips a target with no ports and always renders `ports: Some(..)` (`render.rs`). | `headless_service_without_any_port_is_rejected_not_rendered_portless` (~1653), `target_without_ports_gets_no_egress_rule` (`render.rs` ~738) |
| R5-N3 only first slice port | FIXED | All matching slice ports are collected, sorted, deduplicated. | `named_target_port_collects_the_ports_of_all_slices` (~1674) |
| R5-N4 port guessing | FIXED | Configured ports matching no Service port is `InvalidSpec`. A Service with no ports at all uses the configured ports (explicit operator input). | `configured_ports_that_match_no_service_port_are_an_error` (~1661) |
| R5-N5 init-container pull failures | FIXED | `map_pod_event` also reads `init_container_statuses`. | `init_container_image_pull_failure_is_gone` (~1285). Image-volume pods stuck in ContainerCreating still produce no event (not detectable from container statuses): PARTIAL for that case; the connect timeout ends such runs. |
| R5-N6 missing render tests | FIXED | Tests for unset runtime class (field absent in the JSON) and unset StorageClass. | `unset_storage_class_and_runtime_class_render_as_absent_fields` (`render.rs` ~717) |
| R5-N7 startup check | FIXED | `verify_startup` collects every gap of every namespace (`MissingPermissions { missing }`, each item names its namespace); `secrets get` added to `INSTANCE_PERMISSIONS`. | `startup_reports_gaps_in_every_namespace_not_only_the_first` (`startup.rs` ~536); other startup tests updated. |
| R5-N8 schema issuer kind | FIXED | `sandboxNamespace.issuerKind` enum is now `["", "ClusterIssuer"]` in `values.schema.json`. | `helm lint` (schema), no dedicated test |
| R5-N9 timeouts, debounce | FIXED | `K8sSecretStore::get` uses `K8S_API_TIMEOUT`; the reconcile loop waits `NOTIFY_DEBOUNCE` (1 s) after a notification and drains queued ones (`reconcile.rs` ~448). Shutdown can wait up to that second. | `reading_a_secret_times_out_when_the_api_does_not_answer` (`secrets.rs` ~188); `a_burst_of_notifications_causes_a_single_reconcile_pass` (~1283), verified to fail (6 passes instead of 2) without the drain. |
| R5-N10 Gone after disconnect / restart | PARTIAL | The scheduler does **not** resync active runs from the provider at startup (it relies on events plus timeouts; checked `df-agent-channel/src/scheduler.rs`). `EventTracker` now tracks every pod and Sandbox seen and emits Gone at `InitDone` for those missing from the relist (also never-Running pods, and the Sandbox watch). Not covered: objects that vanished while the control plane was down (nothing known after a restart); those runs end via connect timeout / hard stop. Documented in the README. | `relist_reports_never_running_pods_and_sandboxes_that_vanished` (~1382); existing relist test. |
| R5-N11 third default | FIXED | `| default "cert-manager.io"` removed (`_helpers.tpl`). | helm-unittest, rendered output identical |
| R5-N12 hanging tests | FIXED | The two "sender is gone" loop tests run in real time with a 5 s timeout. Verified: with the `changed()` Err handling removed they fail after 5 s instead of hanging. | `reconcile_loop_stops_when_the_leader_channel_sender_is_gone`, `..._shutdown_sender_is_gone` |
| R5-N13 duplicated block | FIXED | Named template `dark-factory.sandboxConfig` included twice. Rendered chart is identical to before except upstream's resource changes. | helm-unittest, `helm template` diff |
| R5-N14 impl far from type | FIXED | `impl SandboxBackend for KubernetesBackend` moved next to the struct (`main.rs`). | compile |
| R5-N15 data clump | FIXED | `ProvisioningConfig` embeds `sandbox: SandboxSectionConfig` (`k8s/config.rs`); `resolve_cluster_ips(client, instance_name, &SandboxSectionConfig)` (3 parameters). | all df-k8s tests; `from_sandbox_config_copies_section_values` |
| R5-N16 casts / errors | FIXED | `i32::from` instead of `as i32`; `provision` no longer re-wraps a `ProviderError`; the test `unwrap_or_else(panic!)` replaced by `assert!(matches!(..))`. | existing tests |
| R5-N17 weak lag test | FIXED | The test now asserts the 128 newest events arrive, first `72`, last `199`. | `events_stream_survives_lag_and_keeps_later_events` |
| R5-N18 `.sqlx` format | NOT DONE (by instruction) | Only the new file has sqlx-cli 0.9.0 `origin` fields; the other `.sqlx` files were not regenerated. | n/a |
| R5-N19 shutdown vs reconcile pass | FIXED (documented) | Documented on `run_policy_reconcile_loop` and noted for the debounce; a pass is not interrupted, the grace period applies, patches are idempotent. | n/a |

PARTIAL gaps from the round-5 validation table: events (see N10, N5), runtime class `None` and StorageClass (N6), startup "each missing item" (N7), timeouts (N9) are addressed as listed. "Single source of defaults": the chart's template `default` is gone; the runtime image tag `0.1.0` is still written in both `df-core` and `values.yaml` (the chart always renders it); NOT DONE, left as documented duplication.

## Round 6 verification

Run from the repo root, sandbox disabled. Podman: `DOCKER_HOST=unix://$XDG_RUNTIME_DIR/podman/podman.sock`, `TESTCONTAINERS_RYUK_DISABLED=true`.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass |
| `cargo test --workspace` (Podman) | 0 failures: dark-factory 9 passed / 2 ignored; df-agent-channel 69 + 24; df-core 19; df-db 17; df-k8s 47 passed / 3 ignored; df-proto 2; df-test-postgres 6; df-web 3; factory-agent 90 / 1 ignored; fake-acp-agent 8; forgejo-plugin 1 |
| `cargo machete` | pass |
| `cargo deny check` | pass |
| `helm lint` | pass |
| helm-unittest | 15 passed |
| typos | pass |
| strict YAML parse of `values.yaml` | pass, no duplicate keys |
| `cargo llvm-cov -p df-k8s` | 85.09 % lines (3,614 lines, 539 missed), 84.06 % regions; includes inline test code |
| `bazel --bazelrc=.bazelrc.agents build //crates/...` | pass |
| `bazel --bazelrc=.bazelrc.agents test //... --nocache_test_results` with the Podman `--test_env`s | **executed**, 16 of 16 pass (not cached). A following `--test_summary=short` run lists every target PASSED (shown "cached" because it reused that run): `//contract:buf_lint_test`, `//crates/dark-factory:dark_factory_test`, `//crates/df-agent-channel:agent_port_test`, `//crates/df-agent-channel:df_agent_channel_test`, `//crates/df-core:df_core_test`, `//crates/df-db:df_db_test`, `//crates/df-k8s:df_k8s_test`, `//crates/df-proto:df_proto_test`, `//crates/df-test-postgres:df_test_postgres_test`, `//crates/df-web:df_web_test`, `//crates/factory-agent:factory_agent_test`, `//crates/fake-acp-agent:fake_acp_agent_test`, `//deploy:helm_chart_test`, `//deploy:helm_chart_unittest`, `//deploy:images_structure_test`, `//plugins/forgejo:forgejo_plugin_test`. I did not inspect the per-target test logs, so I cannot show that the container-backed targets reached Podman inside the Bazel sandbox. |

Not run: the three `#[ignore]` dev-cluster tests in df-k8s (including the new 4.1 test) and the two in dark-factory. No dependency was added; `Cargo.lock` and the Bazel lock files were not changed by this round. `stash@{0}` was neither applied nor dropped.

---

## Dev-cluster confirmation: task 4.1 (owner, 2026-10-21)

- The owner ran `cargo test -p df-k8s --lib policy_loop_live -- --ignored --nocapture` against the dev cluster.
- Result: `policy_loop_live_reverts_manual_edit_and_adds_new_target` passed in 30.37 s (1 passed, 0 failed).
- R3-N8 is therefore closed. Task 4.1's named verification (a manual edit is reverted and a new target appears within 30 s) now has a passing test.

---

# Round 7 review (2026-10-21)

## What was reviewed
- **Change set:** `git diff HEAD` (HEAD `9c9dd30`) plus the untracked `crates/df-k8s/`, `crates/df-db/src/egress.rs` and `.sqlx/query-3aad8b60….json`.
- **Owner decisions taken as fixed:** runtime class and StorageClass are optional; the CA uses a ClusterIssuer in cert-manager's cluster resource namespace; the dev-cluster tasks are ticked on the owner's statement; tasks 4.1 and 4.2 are confirmed by the owner's dev-cluster runs.

## Commands run by the reviewers

| Command | Result |
|---|---|
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass |
| `cargo test -p df-k8s -p df-core`, run 3× | all pass every time: df-core 19, df-k8s 47 passed / 3 ignored; no flakiness |
| `cargo machete` | pass |
| `cargo test -p df-db -p dark-factory` (Podman) | df-db 17 passed; dark-factory 9 passed, 2 ignored |

Not run in round 7: Bazel, `cargo deny`, Helm, the dev-cluster `#[ignore]` tests.

## Round-6 fix validation

| Item | Verdict | Evidence / gap |
|---|---|---|
| R5-B1 `values.yaml` merge | HOLDS | No markers; the strict YAML parse passes (also re-checked by hand). |
| R5-B2 per-rule resolution | PARTIAL | Holds: rules resolve on their own, a removed rule is always dropped, and one bad rule no longer blocks other runs (`provider.rs:244-277`, `main.rs:120-140`). The `std::sync::Mutex` is never held across an await. **But** the last-good fallback applies to every error, including permanent ones (R7-B1). The cache is never pruned (R7-N1), and the test `last_good_resolution_is_kept_only_while_the_rule_exists` (`provider.rs:1566`) only checks `resolve(&[])`, which is empty whatever the cache holds. |
| R5-N1 headless without `allowDns` rejected | HOLDS | Rejected at resolution, so both provision and reconcile reject it (`provider.rs:100-104`). |
| R5-N2 no rule without ports | HOLDS | `provider.rs:177-184`, `render.rs:532-537`. |
| R5-N3 / N4 port resolution | HOLDS | `provider.rs:145-176`. |
| R5-N5 runtime-image pull failure | PARTIAL | The init-container case is fixed (`init_container_image_pull_failure_is_gone`). An image-volume pod stuck in ContainerCreating still produces no Gone (disclosed). |
| R5-N6 runtime class / StorageClass unset | HOLDS | `unset_storage_class_and_runtime_class_render_as_absent_fields` (`render.rs:717`). |
| R5-N7 startup check names every gap | HOLDS | `startup.rs:244-267`; `secrets get` is checked. |
| R5-N8 issuer kind enum | PARTIAL | `sandboxNamespace.issuerKind` is restricted, but `tls.ca.existingIssuer.kind: Issuer` is still allowed (`values.schema.json:206`) and then renders sandbox `issuerKind: Issuer` (`_helpers.tpl:131,160`). See R7-N6. |
| R5-N9 secret timeout | HOLDS | `secrets.rs:35`. |
| R5-N9 debounce | PARTIAL | Cancel-safe (the sleep runs after `recv`, in the arm body). The test `a_burst_of_notifications_causes_a_single_reconcile_pass` (`reconcile.rs:1283`) still passes with the 1 s window removed (R7-N4). |
| R5-N10 relist handling | PARTIAL | Vanished pods (including never-Running ones) and Sandboxes get Gone at `InitDone`, and the tracker is pruned on Delete and on InitDone. Lost state across a control-plane restart is documented. |
| R5-N11, N13, N14, N17, N19 | HOLDS | — |
| R5-N12 real-time tests with a 5 s timeout | HOLDS | The timeout guards against a hang; it doesn't wait for an effect. Acceptable. |
| R5-N15 config embedding | HOLDS | It leaves duplicated fields behind (R7-N3). |
| R5-N16 casts, re-wrap, test panics | PARTIAL | `i32::from` and the re-wrap removal hold. **The claim is wrong** about the test panic: `unwrap_or_else(|| panic!(…))` is still at `main.rs:970`, and there is a new `other => panic!` at `provider.rs:1498` (R7-N5). |
| R3-N8 task-4.1 test | HOLDS, weakly | See R7-N7. |

## Scenario coverage (spec review)

All 27 scenarios are implemented. Gaps in the tests:

| Scenario | Gap |
|---|---|
| Second run reuses the volume | No test; the live test only runs one run. |
| Run ended (PVC kept) | `test_provider_live_cluster_integration` doesn't assert that the PVC survives (covered by the owner's check 5.1). |
| First run of DF-1 | The PVC name isn't asserted on the mount. |
| Manual edit | The 4.1 test is weak (R7-N7). |

Everything else has a named test or snapshot; see the round-7 spec reviewer's table. Identity, credential and network-policy scenarios are fully covered.

## Blocking

- **R7-B1 ✔ The last-good fallback fails open on permanent errors** (`provider.rs:262-275`). *Both reviewers flagged it; the spec reviewer called it "fix before merge (security)".*
  - On **any** resolution error, the cached resolution is reused for the life of the process. That includes a 404 (Service deleted), `InvalidSpec` (the Service became headless without `allowDns`, or its ports no longer match), and a 403.
  - Selector targets keep egress to pods that still match the old selector.
  - Selectorless targets keep stale `/32` ipBlocks, which reach whatever pod gets those IPs next.
  - Spec: "When … a referenced Service or EndpointSlice changes … SHALL re-render and overwrite the policies of all active runs within 30 seconds." Also: "Headless Services SHALL be rejected unless the entry sets `allowDns`."
  - The cache is per replica, so a new leader renders a different policy.
  - Fix:
    - Treat 404, 403 and `InvalidSpec` as definitive: evict the cache entry and skip the rule.
    - Keep the last good resolution only for timeouts and 5xx.
    - Add tests for a 404 answer and an `InvalidSpec` result (the current test uses a 500).

## Non-blocking

- **R7-N1 Resolver cache never pruned** (`provider.rs:232`). It is keyed by the full rule (namespace, service, ports, `allow_dns`). Edited or removed rules leave dead entries, and a rule that is removed and later re-added gets its old resolution back on its next failure. Fix: `retain` the keys passed in (per-project calls pass different rule sets, so key per project), or accept the growth with a `ponytail:` note. Also fix the weak test at `:1566`.
- **R7-N2 Headless (`allowDns`) targets missing from `NO_PROXY`** (`render.rs:156-171`). Entries are added only when `cluster_ip` is `Some`, so HTTP clients send headless-target traffic to the gateway proxy. Spec: "`NO_PROXY` listing every cluster target of the run." Fix: always add `<svc>.<ns>.svc.<domain>` and `<svc>.<ns>.svc`.
- **R7-N3 Duplicate fields after embedding.**
  - `ProvisioningConfig` has `sandbox.control_plane_ip` (an `Option`) next to the resolved `control_plane_ip`, and the same for `control_plane_tls_server_name`.
  - `instance_namespace` is passed separately to `K8sSandboxProvider::start` and `run_policy_reconcile_loop`, although `config.sandbox.instance_namespace` already holds it.
  - Fix: drop the separate parameters, and name or document which field is authoritative.
- **R7-N4 Debounce window untested.** Fix: send one notification, advance 500 ms, send another, and assert a single pass.
- **R7-N5 Hand-written panics in tests** (`main.rs:970`, `provider.rs:1498`). Fix: `let … else` with `assert!`, or `assert!(matches!(…))`.
- **R7-N6 Chart can still render a sandbox `issuerKind: Issuer`** through `tls.ca.existingIssuer.kind: Issuer`. Fix: fail the template, or require `sandboxNamespace.issuerKind` in that case.
- **R7-N7 The 4.1 test doesn't enforce the 30 s bound or test drift revert reliably** (`reconcile.rs:797,803-807,853-863`).
  - Its deadline is 45 s, and it compares only the number of rules.
  - The owner's 30.37 s run shows the edit was reverted by the loop's immediate first tick racing the manual patch, not by the drift resync. Step 2 (new target) does exercise the resync.
  - Fix: wait for the initial reconcile before the edit, use a 30 s deadline plus a small margin, and compare the rendered rules.
- **R7-N8 Rules resolve one after another** (`provider.rs:245`). The worst case is N × 2 × `K8S_API_TIMEOUT` per provision and per pass. Fine for small allowlists; add a `ponytail:` note.
- **R7-N9 Stale trait doc** (`provider.rs:36-41`). The `ClusterTargetsProvider` doc still says it "must not be answered with a partial list"; the resolver now returns partial lists by design.
- **R7-N10 `tasks.md` wording.**
  - 2.3 says "snapshot tests for every scenario", but "Internet blocked", "Target added mid-run" and "Manual edit" have no snapshot (they are covered elsewhere).
  - 3.1's test doesn't assert that the PVC survives or that the second run reuses it.
  - Fix: adjust the wording, or add the assertions.
- **Carried over, disclosed:**
  - R5-N5: image-volume pull failures produce no Gone.
  - R5-N10: state is lost across a restart.
  - R5-N18: the `.sqlx` format mismatch.
  - The runtime image tag is written in both df-core and `values.yaml`.

## Summary

| | Blocking | Non-blocking |
|---|---|---|
| Round 7 | 1 (R7-B1) | 10 new + 4 carried over |

Worst issue per axis:
- **Spec:** R7-B1, where a deleted or changed Service keeps its old access.
- **Standards:** R7-B1 as well (the fallback doesn't tell transient errors from permanent ones), followed by the overstated R5-N16 claim (R7-N5).

---

# Round 8 fixes

Paths: `k8s` = `crates/df-k8s/src`. Line numbers refer to the tree after round 8.

## R7-B1 last-good fallback fails open: FIXED

- `k8s/provider.rs`: new `ResolveError { Transient, Permanent }` (~76), returned by `resolve_cluster_target`. Classification is typed, no string matching: `is_permanent` checks `kube::Error::Api(status)` with a `4xx` code other than `408`/`429`; timeouts, `5xx`, `408`, `429`, non-API errors and "named port without ready endpoint" are transient; invalid Service spec cases (headless without `allowDns`, unmatched ports, no ports, bad port numbers) are permanent.
- `ClusterTargetResolver::resolve` (~290): transient error reuses the last good resolution (warn); permanent error evicts the cache entry and skips the rule (warn naming the rule); no cache entry skips the rule.
- Tests (`provider.rs`): `a_server_error_reuses_the_last_good_resolution` (~1691), `a_timeout_reuses_the_last_good_resolution` (~1706), `a_404_evicts_the_cache_and_skips_the_rule` (~1721), `a_forbidden_answer_evicts_the_cache_and_skips_the_rule` (~1740), `a_service_that_became_headless_evicts_the_cache_and_skips_the_rule` (~1757, the `InvalidSpec` case). **Confirmed to fail without the fix:** with permanent errors treated like transient ones, exactly the three eviction tests (404, 403, headless) failed.

## R7-N1 cache pruning: FIXED

- Option chosen: **cache per project key** (`HashMap<Option<String>, HashMap<rule, target>>`), pruned after each call to the rules of that call. Reason: per-project calls pass different rule sets, so pruning one global map to a single call's rules would evict the other projects' entries; a per-scope map is simple and bounded by the number of projects. `resolve(scope, rules)` now takes the project key (`main.rs` passes it).
- Tests: `a_rule_that_was_removed_and_added_again_has_no_history` (~1776) replaces the weak test (removal, then re-add while the API fails, must yield nothing); `pruning_one_project_scope_keeps_the_cache_of_the_others` (~1796). **Confirmed:** with the `retain` removed, the first one fails.

## R7-N3 duplicated config fields: FIXED

- `df-core/src/config.rs`: `SandboxSectionConfig` (~203) is now two flattened parts: `SandboxProvisioningConfig` (~217, everything the Kubernetes provider needs, holds all defaults) and `SandboxEndpointsConfig` (~243, the optional `controlPlaneIp`, `gatewayIp`, service names and TLS server name). The YAML (and the chart) is unchanged: one flat `sandbox:` table.
- `ProvisioningConfig` (`k8s/config.rs`) embeds only `SandboxProvisioningConfig`. The resolved `control_plane_ip`, `gateway_ip` and `control_plane_tls_server_name` exist only there; the optional overrides exist only in `SandboxEndpointsConfig` and are read once at startup (`main.rs` `resolve_cluster_ips`, TLS name). No field is stored twice any more.
- The ports are not duplicates inside one struct: `control_plane_port` comes from `server.agentPort`, `gateway_proxy_port` from `gateway.proxyPort` (other config sections); they are only in `ProvisioningConfig` after construction.
- `instance_namespace` parameters are gone from `K8sSandboxProvider::new`/`start`, `run_policy_reconcile_loop`, `reconcile_active_runs` and `render_network_policy`; they use `config.sandbox.instance_namespace`.
- Tests: `sandbox_section_reads_provisioning_and_endpoint_keys_from_one_table`, the updated default tests (df-core); the chart-rendered `config.yaml` was parsed with `InstanceConfig::from_yaml_str` in a throw-away test (removed): flatten works with the real chart output. Refactor, no failing-first test.

## Other items

| Item | Status | What changed | Test |
|---|---|---|---|
| R7-N2 `NO_PROXY` for headless targets | FIXED | `render.rs` ~150: `<svc>.<ns>.svc.<domain>` and `<svc>.<ns>.svc` are always added; the ClusterIP and host alias only when there is one. | `headless_target_is_listed_in_no_proxy_without_a_host_alias` (`render.rs` ~738). **Confirmed to fail** with the names only added for targets with a ClusterIP. |
| R7-N4 debounce window | FIXED | New test; the production code is unchanged. | `notifications_within_the_debounce_window_share_one_pass` (`reconcile.rs` ~1352): second notification half a window after the first, expects one pass. **Confirmed to fail** (3 passes instead of 2) with the 1 s sleep removed. |
| R7-N5 hand-written panics | FIXED | `main.rs` startup test uses `assert!(matches!(err.downcast_ref::<StartupCheckError>(), Some(MissingPermissions { .. })))`. The round-6 claim that this was done was wrong (the replacement had not matched); now verified by `grep panic!`: no `panic!` and no `unwrap_or_else(.. panic!)` in `df-k8s`, `dark-factory`, `df-db/src/egress.rs`, `df-core/src/config.rs` outside generated `crd.rs`. `provider.rs` lag test matches both event variants instead. | the same tests (the `serve_*` one is a dev-cluster test, compiled only) |
| R7-N6 sandbox `Issuer` kind | FIXED | `_helpers.tpl` `dark-factory.sandbox.issuerKind`: the chart fails to render ("sandbox certificates need a ClusterIssuer …") unless the sandbox issuer kind is `ClusterIssuer`; `tls.ca.existingIssuer.kind: Issuer` works when `sandboxNamespace.issuerName` and `issuerKind: ClusterIssuer` are set. Chart README updated. | helm-unittest: `refuses a namespaced Issuer for sandbox certificates`, `accepts a namespaced Issuer for the control plane when sandboxes get a ClusterIssuer`. Not confirmed against the unfixed template. |
| R7-N7 4.1 test | FIXED (written, not run) | `reconcile.rs` `drive_policy_loop` (~830): waits until the loop's first pass reached `get_targets`, then 3 s for its patch, checks the policy equals the expected rendering, edits it by hand, and waits at most `RESYNC_DEADLINE` = 35 s (30 s + margin) for the loop to restore it; then adds a target and waits at most 35 s. The edit can no longer be reverted by the first tick. Compares the rendered egress rules (JSON of the rules produced by `render_network_policy` from the cluster's Sandbox), not their count (`expected_egress` ~776). | `#[ignore]`; compiles, clippy clean. **Not run, cluster untouched.** |
| R7-N8 sequential resolution | FIXED | `ponytail:` note on `ClusterTargetResolver::resolve`; README states the worst case. | n/a |
| R7-N9 trait doc | FIXED | `ClusterTargetsProvider` doc now says an `Err` means the targets could not be determined at all and that the list may be partial by design. | n/a |
| R7-N10 tasks.md / 3.1 | FIXED | 3.1 live test `test_provider_live_cluster_integration` now asserts the volume name `task-df-999`, that the PVC survives the teardown, that run 2 mounts the same claim and that there is one PVC per task; it cleans up. `tasks.md` 2.3 wording corrected (the three scenarios are covered by the default-deny policy, reconcile tests and the 4.1 live test). | `#[ignore]`; compiles. **Not run.** |

## Dev-cluster commands to re-run

- Task 4.1 (changed): `cargo test -p df-k8s --lib policy_loop_live -- --ignored --nocapture`. Takes about 1 minute. It creates PVC `task-df-889`, Sandbox/pod `run-df-889-1` and NetworkPolicy `run-df-889-1` in `dark-factory-dev-sandboxes`, patches that policy once, and removes everything afterwards (also on failure). Prerequisites as in the round-6 section: `KUBECONFIG` for the dev cluster, rights to create/patch/delete PVCs, Sandboxes and NetworkPolicies there, agent-sandbox controller running, `alpine:latest` pullable.
- Task 3.1 (changed): `cargo test -p df-k8s --lib test_provider_live_cluster_integration -- --ignored --nocapture`. Creates PVC `task-df-999` and Sandboxes `run-df-999-1`, `run-df-999-2` (and the network policies) in `dark-factory-dev-sandboxes`; removes the sandboxes and the PVC at the end. If it aborts, clean up with `kubectl -n dark-factory-dev-sandboxes delete sandbox/run-df-999-1 sandbox/run-df-999-2 networkpolicy/run-df-999-1 networkpolicy/run-df-999-2 pvc/task-df-999`.

## Not done

- Carried over, disclosed earlier: image-volume pull failures without a Gone event (R5-N5), state lost across a control plane restart (R5-N10), `.sqlx` format mismatch (R5-N18), runtime image tag written in `df-core` and `values.yaml`.
- R7-N6 template guard and R7-N5/N3 changes have no failing-first run against the old code (see above); the other regression tests were confirmed to fail without their fix.

## Round 8 verification

Podman: `DOCKER_HOST=unix://$XDG_RUNTIME_DIR/podman/podman.sock`, `TESTCONTAINERS_RYUK_DISABLED=true`.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass |
| `cargo test --workspace` (Podman) | 0 failures: dark-factory 9 passed / 2 ignored; df-agent-channel 69 + 24; df-core 20; df-db 17; df-k8s 55 passed / 3 ignored; df-proto 2; df-test-postgres 6; df-web 3; factory-agent 90 / 1 ignored; fake-acp-agent 8; forgejo-plugin 1 |
| `cargo machete` | pass |
| `cargo deny check` | pass |
| `helm lint` | pass |
| helm-unittest | 17 passed |
| strict duplicate-key check of `values.yaml` | pass |
| typos | pass |
| `cargo llvm-cov -p df-k8s` | 84.17 % lines (3,866 lines, 612 missed), 82.66 % regions; includes inline test code |
| `bazel --bazelrc=.bazelrc.agents build //crates/...` | pass |
| `bazel --bazelrc=.bazelrc.agents test //... --nocache_test_results` with the Podman `--test_env`s | 16 of 16 executed (none cached) and passed: buf_lint, dark_factory (1.3 s), agent_port, df_agent_channel, df_core, df_db (2.5 s), df_k8s, df_proto, df_test_postgres (2.3 s), df_web, factory_agent, fake_acp_agent, helm_chart_test, helm_chart_unittest, images_structure, forgejo_plugin. I did not inspect the per-target logs to prove the container tests used Podman inside the Bazel sandbox. |

No dependency changed; `Cargo.lock` and the Bazel lock files were not touched by this round. The dev-cluster `#[ignore]` tests were not run.

# Round 8, follow-up: task 4.1 dev-cluster failure (2026-10-21)

- **Symptom:** `policy_loop_live_reverts_manual_edit_and_adds_new_target` failed after 8 s. The policy was missing the test target's rule (`nativelink/scheduler:50051`); only the control-plane and gateway rules were left.
- **Cause 1: the test conflicted with the deployed control plane.** The live tests used the instance name `homelab-dev`, the same as the deployment in `dark-factory-dev`. That instance holds the Lease `homelab-dev-leader`, so its reconcile pass also rewrote the test's policy, using the DB's targets (none). A longer timeout would not fix this.
  - **Fix:** all three live tests (`provider.rs` 3.1, `reconcile.rs` `test_reconciliation_live_cluster` and 4.1) now use the instance name `df-live-test`.
- **Cause 2 (production bug, found in the deployed control plane's logs):** it logged `became leader; running initial policy reconciliation` every 5 s. `run_leader_election` called `watch::Sender::send(is_leader)` on every Lease renewal, and `send` marks the value as changed even when it is unchanged. So every renewal triggered a full reconcile pass.
  - **Fix:** a new `publish_leadership` helper uses `send_if_modified` and wakes receivers only on a real change. It is used at all three send sites.
  - **Test:** `publishing_unchanged_leadership_does_not_wake_receivers`. Confirmed to fail with the old `send`.
- **Verification:** `cargo fmt`, `cargo clippy -p df-k8s --all-targets -D warnings` are clean, and `cargo test -p df-k8s` gives 56 passed, 3 ignored. The live tests must be re-run by the owner.

## Follow-up: live-test pods stuck in `ContainerCreating` (2026-10-21)

- **Symptom:** during 4.1, pod `run-df-889-1` stayed in `ContainerCreating`. The csi-driver logged `waiting for request: certificaterequest … not found`.
- **Cause:** the three live tests used issuer `dark-factory-ca-issuer` (the df-core default). In the dev cluster the ClusterIssuer is `dark-factory-dev-dark-factory-ca-issuer`. The CertificateRequest was never signed, so the csi-driver gave up after 60 s, deleted it and retried.
- **Fix:** the tests now use `dark-factory-dev-dark-factory-ca-issuer`. Production is unaffected: the chart renders `issuerName` explicitly.
- **Still open:** the tests deliver `alpine:latest` as the runtime image, so `/opt/factory/factory-agent` doesn't exist. After the mount the container will fail to start. No live test asserts `Running` or `Started`. A test that does needs a real runtime image.
- **Minor:** the df-core default `issuer_name: "dark-factory-ca-issuer"` doesn't match the chart's naming (`<release-ns>-<fullname>-ca-issuer`).

# Round 9 review (2026-10-21)

- **Scope:** `git diff HEAD` plus the untracked files (`crates/df-k8s/`, `crates/df-db/src/egress.rs`, `.sqlx/query-3aad8b60….json`).
- **Reviewers:** Standards and Spec reviewers on Sonnet. Their claims were spot-checked against the source.
- **Reviewer checks:** `cargo clippy -p df-k8s --all-targets -D warnings` is clean, and `cargo test -p df-k8s --lib` gives 56 passed, 3 ignored. Helm, Bazel and the live tests were not re-run in this round.

## Verdict on the round-8 fixes and follow-ups

All hold.
- **R7-B1:** `is_permanent` returns true for 4xx other than 408/429 (`provider.rs:87`). Transient errors reuse the cache; permanent errors evict and skip.
- **R7-N1:** the cache is pruned per scope.
- **R7-N3:** one source per value.
- **`publish_leadership`:** all three send sites use it. Acquiring leadership still triggers the initial pass.
- **Live-test instance and issuer:** both changed in all three tests.
- **R7-N2, N4, N5, N6, N8, N9:** hold.
- **R7-N7 and N10:** live and doc items; not re-verified.

## Blocking

None.

## Non-blocking

- **R9-N1 (Spec): a transient resolution failure at provision time silently drops a target for the whole run.**
  - Spec: "the pod SHALL get `hostAliases` mapping … to the Service's ClusterIP".
  - When a resolution is transient and has no cached entry, the rule is skipped (`provider.rs:322`).
  - The pod starts without the alias or the `NO_PROXY` entry. `hostAliases` can't be changed later, while the reconcile pass does add the policy rule afterwards.
  - The failure is closed but silent.
  - Option: fail `provision` in this case, so the run is retried instead of starting degraded.
- **R9-N2 (Spec): a named `targetPort` with no ready endpoint is Transient (`provider.rs:200`).**
  - Without a cache entry the rule is skipped, so a target whose pods aren't ready yet is briefly unreachable.
  - Acceptable: it fails closed, and the EndpointSlice watch triggers a pass once endpoints appear.
- **R9-N3 (Spec, informational): nothing in this change publishes the `egress_policy` notification.**
  - The schema has only `egress_policy_version_seq`; there is no trigger and no NOTIFY.
  - The writer belongs to the `iter1-egress-gateway` change. Until then, DB edits reach running policies through the 30 s resync, which is still within the spec's bound.
- **R9-N4 (Standards, hard rule; AGENTS.md "Don't silently drop errors with `let _ =` unless … say so in a short comment"):**
  - `main.rs:77`: `install_default`, already installed.
  - `main.rs:241,249`: `try_send`, where a full channel already means a pass is pending.
  - These lack the comment. The reviewer also flagged `provider.rs:571` and `reconcile.rs:521`, but both already have comments, so that part was overstated.
- **R9-N5 (Standards, Duplicated Code): the three live tests each build the same `ProvisioningConfig` literal, including the issuer name that broke once already.**
  - Extract a single `live_config()` test helper.
- **R9-N6 (Standards, Data Clumps, judgement call): `ProvisioningConfig::from_sandbox_config` takes 7 positional arguments**, with two `IpAddr` and port pairs side by side that are easy to swap. Fine for now.

## Still open, carried over

R5-N5, R5-N10 (also covers a new leader starting with an empty last-good cache), R5-N18, the runtime image tag written twice, the df-core default issuer name, and the alpine runtime image in the live tests.

## Summary

- **Standards:** 3 non-blocking items. The worst is R9-N4, the uncommented `let _ =` in `main.rs`.
- **Spec:** 3 non-blocking items. The worst is R9-N1, a target silently missing after a transient error at provisioning.
- **Blocking:** none.

# Round 9 fixes

- **R9-N4:** added "ignored on purpose" comments to `crates/dark-factory/src/main.rs` at `install_default` (:77), both `try_send` sites (:246, :256), the two health-response `write_all` calls (~:618, ~:642) and `ctrl_c` (~:671). The other production `let _ =` in `df-k8s/src` and `df-db/src/egress.rs` were already commented or are in tests.
- **R9-N5:** new `crates/df-k8s/src/test_support.rs` (`#[cfg(test)]`, declared in `lib.rs`) with `live_config()`, which keeps the doc comment on the instance name. Used by `provider.rs` and both live tests in `reconcile.rs`. The configs differed on purpose: `test_reconciliation_live_cluster` overrides `runtime_image` and `runtime_delivery` with struct update syntax. The policy-loop test had defaults for storage class, runtime class and others; it now uses the full live values, since it creates no pods or PVCs from them.
- **R9-N6:** `config.rs` gains `ControlPlaneEndpoint {ip, port, tls_server_name}` and `GatewayEndpoint {ip, proxy_port}`, exported from `lib.rs`. `from_sandbox_config` now takes 4 arguments. `ProvisioningConfig` keeps its flat fields (smaller diff). Callers updated: `main.rs`, the `test_config()` helpers in `provider.rs` and `reconcile.rs`, and the `config.rs` test.
- **Verification (all run):** `cargo fmt --all` and `cargo clippy --all-targets --all-features -- -D warnings` were clean. `cargo test -p df-k8s` gave 56 passed, 3 ignored. `cargo test -p dark-factory` gave 9 passed, 2 ignored, with container tests run through the Podman socket. `cargo machete` found nothing. The live tests were not run.

## Round 9 fixes: verification (2026-10-21)

- **Checked by hand:** clippy for the whole workspace with `-D warnings` is clean, and `cargo test -p df-k8s` gives 56 passed, 3 ignored.
- **R9-N4:** the `let _ =` at `main.rs:520` is commented. The one at `main.rs:818` is in the test module.
- **R9-N5:** all three live tests call `crate::test_support::live_config()`.
- **R9-N6:** `from_sandbox_config(instance_name, &sandbox, ControlPlaneEndpoint, GatewayEndpoint)`.
- **Changed 4.1 test config:** `policy_loop_live_reverts_manual_edit_and_adds_new_target` now runs with the full live config, including `runtimeClassName: kata` and StorageClass `longhorn`. Before it used the defaults. The test checks only the NetworkPolicy, but re-run it once.
- **Moved to `docs/tech_debt.md`:** R9-N1 → TD-28, R9-N2 → TD-29, R9-N3 → TD-30, R5-N5 → TD-31, R5-N10 → TD-32, R5-N18 → TD-33, runtime image tag → TD-34, default issuer name → TD-35, live tests without a running sandbox → TD-36.
- **Open blocking items:** none.
