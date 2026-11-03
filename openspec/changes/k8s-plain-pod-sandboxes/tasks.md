# Tasks

No container-backed tests (`df_test_postgres`) are involved; `df-k8s` unit tests run with `cargo test -p df-k8s --lib`. The `#[ignore]` live tests need the dev cluster.

## 1. Render a Pod

- [ ] 1.1 Write failing render tests, then replace `render_sandbox` with `render_pod` returning a `k8s_openapi` `Pod` (design D2, D8). Verify with `cargo test -p df-k8s --lib render`. The tests cover:
  - name `run-<key>-<n>`, labels, volumes, security context and runtime class as before (spec "Sandbox per run", "Sandbox runtime class", "Restricted pod security", "Image volumes only");
  - `restartPolicy: Never` and no owner reference;
  - no harness volume when the project sets an adapter command.
- [ ] 1.2 Change `render_network_policy` to take the Pod's name and UID and emit an owner reference `v1`/`Pod` with `controller: true` and no `blockOwnerDeletion` (design D3); write the failing test first. Accept the updated snapshots in `render/snapshots/` after checking that only the wrapper and owner kind changed. Verify with `cargo test -p df-k8s --lib render`

## 2. Provider on Pods

- [ ] 2.1 Write failing tests for `map_pod_event`: phase `Succeeded` emits `Gone` with reason `pod completed`; existing cases unchanged (spec "Sandbox events"). Then implement. Verify with `cargo test -p df-k8s --lib provider`
- [ ] 2.2 Write failing identity tests: a pod without the instance label or without a valid run id label resolves to nothing; a labelled pod with matching UID and IP resolves (spec "Identity resolution"). Then drop the Sandbox store from `resolve_identity` and `K8sSandboxProvider` (design D4). Verify with `cargo test -p df-k8s --lib provider`
- [ ] 2.3 Reduce `forward_events` to the Pod stream and remove `sandbox_event` and the Sandbox reflector, keeping the relist behaviour; adapt the event tests (pod delete → `Gone`, relist reports missing runs as gone, no `Delete` during relist) first. Verify with `cargo test -p df-k8s --lib provider`
- [ ] 2.4 `provision`: create the Pod (409 `AlreadyExists` = success, quota 403 = `Capacity`), then apply the policy with the new Pod's UID (design D1). `teardown` and `sandbox_exists` list and delete Pods by instance and run-id labels (design D5). Write failing tests for the error mapping (`AlreadyExists`, quota, other 4xx) first. Verify with `cargo test -p df-k8s --lib provider`

## 3. Reconcile and startup

- [ ] 3.1 Switch the policy reconcile loop to list the instance's Pods, skip pods with a deletion timestamp and render each policy from the Pod (design D6); adapt `reconcile/tests.rs` first, including the live tests' setup (create a Pod instead of a Sandbox). Verify with `cargo test -p df-k8s --lib reconcile`
- [ ] 3.2 Replace the `agents.x-k8s.io` entries of `SANDBOX_PERMISSIONS` with `pods` create/delete (keeping get/list/watch) (design D7); update the startup tests first (spec "Startup verification": missing `pods` create is named, no agent-sandbox permission is required). Verify with `cargo test -p df-k8s --lib startup`
- [ ] 3.3 Delete `crates/df-k8s/src/crd.rs`, its module declaration and the `crates/df-k8s/src/crd.rs` entry in `[workspace.metadata.file-length.allow]` of the root `Cargo.toml`; fix the doc comments in `crates/df-core/src/seams.rs` and `crates/dark-factory/src/sandbox.rs`. Verify with `grep -rn "agents.x-k8s\|crd::" crates` returning nothing, `cargo build --workspace`, `cargo xtask check-file-length` and `cargo machete`

## 4. Deployment and e2e

- [ ] 4.1 Helm: replace the `agents.x-k8s.io` rule in `templates/rbac.yaml` with `pods` create/get/list/watch/delete; update `tests/rbac_test.yaml` first. Verify with the chart's helm-unittest run (as in `lefthook.yml`)
- [ ] 4.2 Remove the agent-sandbox install and wait from `deploy/e2e/e2e-local.sh` (`agent_sandbox_version`, `kc apply`, `agent-sandbox-system` wait). Verify with `shellcheck` and, where a container socket is available, `just e2e-local`; otherwise state that the e2e run was not executed

## 5. Documentation

- [ ] 5.1 Amend ADR-0030 (dated note: the Kubernetes provider creates plain Pods; agent-sandbox removed with the reasons from proposal.md) and update ADR-0001 and ADR-0003 (no kopium-generated types). Verify by reading the ADRs for remaining agent-sandbox mentions
- [ ] 5.2 Update `design/tech-stack.md`, `design/deployment.md` (prerequisites, RBAC table, Migration Plan steps from design.md), `design/agent-run.md` (glossary, §5.3, sidecar note), `design/egress.md` (policy owner), `product/context.md`, `docs/demo.md`, `crates/df-k8s/README.md` and `deploy/helm/dark-factory/README.md`; remove the backlog row "Replace agent-sandbox with plain scheduled Pods" from `product/iterations/roadmap.md`. Verify with `grep -rn -i "agent-sandbox\|agents.x-k8s" --exclude-dir=openspec --exclude-dir=target .` showing only historical mentions (archived changes, `review_findings.md`, the ADR-0030 amendment)

## 6. Integration checks

- [ ] 6.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test -p df-k8s --lib`, `cargo fmt --all --check` and `bazel --bazelrc=.bazelrc.agents build //crates/df-k8s/...`; all pass
- [ ] 6.2 On the dev cluster (when available): follow the migration plan, then run `cargo test -p df-k8s --lib -- --ignored --nocapture` and one run end to end; confirm the pod has `restartPolicy: Never`, the policy is owned by the pod and disappears when the pod is deleted by hand, and the run then fails `sandbox_lost`. Report if the cluster was not available
