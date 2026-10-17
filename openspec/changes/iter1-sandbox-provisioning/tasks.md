# Tasks

## 1. Spikes on the dev cluster (iteration-1 §7)

- [ ] 1.1 Verify image volumes with Talos containerd and Kata (pod with an image volume at `/opt/factory`, runtime class `kata`); record the result and the chosen `runtimeDelivery` in `crates/df-k8s/README.md`
- [ ] 1.2 Verify cert-manager csi-driver volumes inside a Kata pod (per-pod URI SAN substitution, rotation); record the result
- [ ] 1.3 Verify the pinned agent-sandbox `Sandbox` pod template passes through image volumes, CSI volumes, PVCs, `hostAliases`, runtime class and `automountServiceAccountToken: false`; record the result
- [ ] 1.4 Verify NetworkPolicy enforcement and preserved pod source IPs for Kata pods (connection from sandbox to a test server logs the pod IP; blocked egress fails); record the result

## 2. Types and rendering

- [ ] 2.1 Create `df-k8s`, generate and check in kopium types for the pinned `Sandbox` CRD; verify they deserialize the CRD's example object
- [ ] 2.2 Implement `render_pvc` and `render_sandbox` test-first (design D1–D3, D6, security context, env wiring, `hostAliases`); verify snapshot tests and the "Sandbox spec" scenario
- [ ] 2.3 Implement `render_network_policy` test-first (control plane, gateway, cluster targets with target-port mapping, selector-less Services as `/32`, optional DNS); verify snapshot tests for every scenario in `specs/run-network-policy/spec.md`

## 3. Provider

- [ ] 3.1 Implement `ensure_task_volume`, `provision`, `teardown` via server-side apply/delete; verify against the dev cluster with an ignored-by-default integration test (`--ignored`, `KUBECONFIG`) incl. idempotent teardown
- [ ] 3.2 Implement reflectors, `resolve_identity` and `events()` (design D4); verify unit tests with a seeded store for every scenario in `specs/sandbox-identity/spec.md` and event mapping tests for Running, deleted and image-pull failure
- [ ] 3.3 Implement the startup check (namespace exists, `SelfSubjectAccessReview` per required verb/resource); verify the "missing permission" scenario against a fake API server (`tower-test`)
- [ ] 3.4 Implement `SecretStore` (instance namespace, redacting wrapper); verify "rotated Secret", "missing key" and the debug-logging scenario

## 4. Network policy reconciliation

- [ ] 4.1 Implement Lease leader election and the reconcile loop (notifications, Service/EndpointSlice watches, 30 s resync); verify with an integration test that a manual policy edit is reverted and a new target appears within 30 s
- [ ] 4.2 Wire `df-k8s` into `dark-factory serve` (provider, secret store, startup check, leader loop); verify `serve` against the dev cluster refuses to start with a missing Role and starts with the chart's RBAC

## 5. Verification

- [ ] 5.1 On the dev cluster, provision a sandbox with the test project image and `factory-agent` and verify: no SA token, `/opt/factory` read-only, certificate SAN matches the pod, direct internet blocked, proxy and control plane reachable by IP, NativeLink target reachable by name
- [ ] 5.2 Document provisioning config keys (StorageClass, PVC size, runtime class, issuer, `runtimeDelivery`, cluster domain) in `crates/df-k8s/README.md` and verify `cargo llvm-cov -p df-k8s` reports ≥ 70 % line coverage
