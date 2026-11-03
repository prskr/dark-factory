# Review log: k8s-plain-pod-sandboxes

- BASE: 09bdfdd42b78e9389791f3eccb33f5758db7689e
- Store: none
- Status: done
- Implementer agent: a08aca43a04034200

## Round 1 (2026-11-03)

- Implementer: 30a77e0 (a97711b, b98ae2d, 30a77e0), tasks 14/16 (4.2 partial: script edited + shellcheck, `just e2e-local` failed: kind node cannot start under Podman; 6.2 not run: dev cluster not touched without go-ahead), checks fmt/clippy/machete/df-k8s tests 81 ok/build/file-length/openspec validate/helm-unittest 85/shellcheck/bazel build+lint df-k8s ok, container tests: df-k8s has none; 3 `#[ignore]` live-cluster tests not run
- Reviewed range: BASE...30a77e0

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P3 | Spec | Blocking | `crates/df-k8s/src/provider.rs:704-737` | Teardown scenarios "Run ended"/"Already deleted" only covered by an `#[ignore]` live test | fix: mock tests (list+DELETE pod, 404 ok, empty list ok) | fixed (283e602) |
| P1 | Spec | P1 | `design/agent-run.md:485` | Sidecar note still mentions the `Sandbox` pod template | fix-now | fixed (283e602) |
| P2 | Spec | P1 | `docs/demo.md:41` | e2e-local download list still names agent-sandbox | fix-now | fixed (283e602) |
| S1 | Standards | P1 | `crates/df-k8s/src/render/tests.rs:184` | `test_render_pod_spec_scenario` not named after behavior | fix-now | fixed (283e602) |
| S2 | Standards | P1 | `crates/df-k8s/src/provider/tests.rs:669` | `test_resolve_identity_scenarios` topic name | fix-now | fixed (283e602) |
| S3 | Standards | P1 | `crates/df-k8s/src/provider/tests.rs:659` | `SANDBOX_NS` const mid-module, literal duplicated in `store_with_pod` | fix-now | fixed (0949ed6) |
| S4 | Standards | P1 | `crates/df-k8s/src/reconcile/tests.rs` (`reconcile_owns_each_policy_by_its_pod_and_skips_deleting_pods`) | `body`/`query` hold method/URI | fix-now | fixed (283e602) |
| S5 | Standards | P1 | `crates/df-k8s/src/reconcile/tests.rs` (~862) | Responder loop duplicates `spawn_api_responder` | fix-now | fixed (283e602) |
| S7 | Standards | P1 | `crates/df-k8s/src/provider/tests.rs` (`provision_with_pod_create_answered`) | Doc comment misdescribes return value | fix-now | fixed (283e602) |
| S8 | Standards | P1 | `crates/df-k8s/src/provider.rs:162` | `resolve_identity_from_stores` has one store now | fix-now | fixed (283e602) |
| S6 | Standards | P1 | `crates/df-k8s/src/provider/tests.rs:1188` | `RunSandboxSpec` literal duplicated in two tests | fix-now (user) | fixed (283e602) |
| S9 | Standards | P1 | `crates/df-k8s/src/reconcile.rs` (`render_run_policy`) | Pod name read twice | fix-now (user) | fixed (283e602) |
| S10 | Standards | P3 | `crates/df-k8s/src/provider/tests.rs` (~1030) | Missing blank line between tests | cosmetic | cosmetic |
| P4 | Spec | Rejected | provision/teardown | E2E GC + no-CRD scenarios only verifiable live | rejected (covered by open task 6.2/4.2, not a code defect) | rejected |
| — | Security | — | — | 0 findings | — | — |

User decision: fix blocking + P1, plus S6 and S9 (moved to P1); S10 stays cosmetic.

## Round 2 (2026-11-03)

- Implementer: 283e602, tasks 14/16 (4.2, 6.2 unchanged), checks fmt/clippy/machete ok, df-k8s lib tests 84 ok / 3 ignored (live), container tests: none in df-k8s
- Reviewed range: 30a77e0...283e602

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-S1 | Standards | P1 | `crates/df-k8s/src/provider/tests.rs:65` | `SANDBOX_NS` still between helpers, not after `use` (S3 incomplete) | fix-now | fixed (0949ed6) |
| R2-S5 | Standards | P1 | `crates/df-k8s/src/reconcile/tests.rs:566,620,791,831` | `spawn_api_responder` JoinHandle dropped by four callers | fix-now | fixed (0949ed6) |
| R2-S6 | Standards | P1 | `crates/df-k8s/src/reconcile/tests.rs:523-550` | S5 refactor dropped the GET path assertions (namespaced pod list) | fix-now | fixed (0949ed6) |
| R2-P1 | Spec | P1 | `crates/df-k8s/src/provider/tests.rs:1311-1349` | Teardown tests don't fail cleanly on an extra request (e.g. PVC delete) | fix-now | fixed (0949ed6) |
| R2-S2 | Standards | P3 | `crates/df-k8s/src/provider/tests.rs:1311` | `teardown_answered` returns a 3-tuple, unlike `Provisioned` | cosmetic | cosmetic |
| R2-S3 | Standards | P3 | `crates/df-k8s/src/provider/tests.rs:1318,1359` | Run id computed twice by convention | cosmetic | cosmetic |
| R2-S4 | Standards | P3 | `crates/df-k8s/src/provider/tests.rs:1340-1374` | `delete_status` meaningless for empty list; reason always NotFound | cosmetic | cosmetic |
| — | Security | — | — | 0 findings | — | — |

User decision: fix P1 (R2-S1, R2-S5, R2-S6, R2-P1); R2-S2..S4 cosmetic.

## Round 3 (2026-11-03)

- Implementer: 0949ed6, tasks 14/16 (4.2, 6.2 unchanged), checks fmt/clippy/machete ok, df-k8s lib tests 84 ok / 3 ignored (live); extra-request check verified by a temporary PVC DELETE (not committed)
- Reviewed range: 283e602...0949ed6

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R3-S1 | Standards | P1 | `crates/df-k8s/src/provider/tests.rs:1356-1358` | Hand-written `if let … { panic! }` guard instead of `assert!` (AGENTS.md Tests) | fix-now | fixed (c707612) |
| R3-S2 | Standards | P3 | `crates/df-k8s/src/provider/tests.rs:1355,1359` | Needless `let mut call = call; (&mut call).await` | cosmetic | fixed (c707612) |
| R3-S3 | Standards | P3 | `crates/df-k8s/src/reconcile/tests.rs:624,656,839,883` | Same comment ×4; belongs in `spawn_api_responder` doc (which doesn't mention its assertions) | cosmetic | fixed (c707612) |
| — | Spec | — | — | 0 findings; R2-P1 verified | — | — |
| — | Security | — | — | 0 findings | — | — |

User decision: fix R3-S1, R3-S2, R3-S3, verify with checks only (no further review round), then finish.

## Finish (2026-11-03)

- Round 3 fixes (R3-S1..S3): c707612; fmt, clippy, machete, df-k8s lib tests 84 ok / 3 ignored. No further review round (user decision).
- Bazel: `bazel --bazelrc=.bazelrc.agents build --lockfile_mode=error //crates/... //plugins/...` ok.
- E2E (`just e2e-local`, Podman/kind after the user's Podman fix): 7/9 in the full run; `stopping_a_run_makes_the_task_wait_and_cancelling_it_closes_its_requests` passes alone (failed in the full run only after the timed-out rate-limit test, at its first UI wait, before any stop). `rate_limited_task_resumes_at_the_retry_time_without_a_message` fails for a pre-existing reason outside this change: a scheduler resume of a `rate_limited` task starts a run without a prompt and the runtime ends it at once (`factory-agent/src/runner.rs` `pre_turn_end`, "no queued prompts"); the user fixes it in a separate change, not tracked here. Task 4.2 ticked.
- Task 6.2: the user confirmed a run on the dev cluster works; ticked.
- Container-backed tests: df-k8s has none; no df_test_postgres tests were affected.
- Tech debt: none. Deferred work: Proxmox/VM provider seam not added to the roadmap (user tracks it).
- Status: done (15/15 tasks).

## Correction after archive (2026-11-03)

- The finish step ran only `bazel build //crates/... //plugins/...`; no `bazel test //...` ran during the change. `just bazel-test` (user) showed `//deploy:helm_chart_test` failing: KubeLinter `access-to-create-pods` on the sandbox RoleBinding (pods create, design D7).
- Fix: scoped `ignore-check.kube-linter.io/access-to-create-pods` annotation with reason on that RoleBinding in `deploy/helm/dark-factory/templates/rbac.yaml`. `just bazel-test`: 41/41 pass.
