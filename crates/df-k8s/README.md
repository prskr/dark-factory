# df-k8s

Kubernetes integration crate for Dark Factory: `SandboxProvider` implementation (managing agent-sandbox `Sandbox` CRDs and persistent task volumes), identity resolution from pod informers, per-run `NetworkPolicy` rendering and reconciliation, and `SecretStore` implementation reading Kubernetes Secrets.

## Spike Results (Iteration 1 §7)

Spikes 1.1-1.4 were run on the development cluster (Kata, cert-manager CSI driver, agent-sandbox, Cilium) and verified by the project owner (statement of 2026-10-21; the commands were not run or recorded by the implementing agent).

| Spike | Result |
|---|---|
| 1.1 Image volumes | An `image` volume mounted read-only at `/opt/factory` works with containerd and Kata. Image volumes are the only delivery of the runtime and the harness; the cluster must run Kubernetes 1.36 or newer (image volumes GA). |
| 1.2 cert-manager CSI driver | `csi.cert-manager.io` volumes mount at `/run/factory/tls` (`tls.crt`, `tls.key`, `ca.crt`) and substitute `${POD_NAMESPACE}`, `${POD_NAME}`, `${POD_UID}` into the URI SAN. |
| 1.3 `Sandbox` pod template | The pinned `agents.x-k8s.io/v1beta1` controller passes `runtimeClassName`, `automountServiceAccountToken: false`, `hostAliases`, the restricted `securityContext`, `image`/`csi`/PVC/`emptyDir` volumes to the pod. |
| 1.4 NetworkPolicy | Policies are enforced and pod source IPs are preserved. |

## Provisioning Configuration Keys

All defaults live in one place: `impl Default for df_core::config::SandboxSectionConfig`. A key missing from the `sandbox` table takes its value from there; `ProvisioningConfig` is built from that section in code and has no defaults of its own. The Helm chart renders every key below, so its `values.yaml` is what applies in a chart install.

An empty string for `storageClass` or `runtimeClassName` counts as unset.

| Key | Required | Default | Effect |
|-----|----------|---------|--------|
| `sandbox.namespace` | no | `dark-factory-sandboxes` | Namespace where sandboxes and task PVCs are provisioned. |
| `sandbox.instanceNamespace` | no | `dark-factory` | Namespace of the control plane release (Lease, Service lookups, Secrets). |
| `sandbox.serviceAccountName` | no | `dark-factory-sandbox` | ServiceAccount of sandbox pods (`automountServiceAccountToken: false`). |
| `sandbox.storageClass` | no | unset | StorageClass of task PVCs. Unset: no `storageClassName` is rendered and the cluster's default StorageClass applies. |
| `sandbox.pvcSize` | no | `20Gi` | Storage request of task PVCs. |
| `sandbox.runtimeClassName` | no | unset | Runtime class of sandbox pods. Unset: no `runtimeClassName` is rendered and the cluster's default runtime applies. Set `kata` for microVM isolation. |
| `sandbox.issuerName` | no | `dark-factory-ca-issuer` | cert-manager issuer for sandbox client certificates. |
| `sandbox.issuerKind` | no | `ClusterIssuer` | Kind of that issuer. Sandbox certificates are requested from the sandbox namespace, so this must be a `ClusterIssuer`. |
| `sandbox.issuerGroup` | no | `cert-manager.io` | API group of that issuer. |
| `sandbox.certificateDuration` | no | `6h` | Lifetime of sandbox client certificates. |
| `sandbox.certificateRenewBefore` | no | `2h` | Renewal window of sandbox client certificates. |
| `sandbox.runtimeImage` | no | `ghcr.io/prskr/dark-factory/runtime:0.1.0` | Runtime image delivered to `/opt/factory`. |
| `sandbox.clusterDomain` | no | `cluster.local` | Cluster DNS domain for Service FQDN host aliases. |
| `sandbox.logLevel` | no | `info` | `RUST_LOG` directive set in the sandbox runtime's environment. Passed on as is; the runtime falls back to `info` (with a warning) for an invalid value. |
| `sandbox.controlPlaneIp` | no | resolved | ClusterIP sandboxes use to reach the agent port. Unset: resolved from the control plane Service at startup. |
| `sandbox.gatewayIp` | no | resolved | ClusterIP of the egress gateway proxy. Unset: resolved from the gateway Service at startup. |
| `sandbox.controlPlaneServiceName` | no (the chart sets it) | fallback list | Service to resolve the control plane IP from. **Fallback when unset:** tries `<instanceName>-control-plane`, then `dark-factory-control-plane`, in `sandbox.instanceNamespace`. |
| `sandbox.gatewayServiceName` | no (the chart sets it) | fallback list | Service to resolve the gateway IP from. **Fallback when unset:** tries `<instanceName>-gateway`, `<instanceName>-egress-gateway`, then `dark-factory-gateway`. |
| `sandbox.controlPlaneTlsServerName` | no (the chart sets it) | `<instanceName>-control-plane` | Server name sandboxes verify on the agent endpoint. It must be a DNS name of the control plane certificate. |
| `clusterTargets.anyNamespace` | no | `false` | The control plane may read Services and EndpointSlices in every namespace. |
| `clusterTargets.namespaces` | no | `[]` | Namespaces it may read when `anyNamespace` is `false`. |

Harness image (not part of the `sandbox` table; `dark-factory` passes it on as `ProvisioningConfig.harness_images`):

| Key | Required | Default | Effect |
|-----|----------|---------|--------|
| `harness.claudeCode.image` | no | `ghcr.io/prskr/dark-factory/claude-harness:0.95.0` | Image of the `claude-code` harness (adapter, Node.js, Claude Code CLI), mounted read-only at `/opt/harness/claude-code` (volume `harness-claude-code`) unless the run's project sets an adapter command. A blank value counts as not configured: a run that needs the image then fails provisioning with a message naming the harness. |

The sandbox also gets `DF_LOG_FORMAT`, set to the control plane's own log format (`text` or `json`), next to `RUST_LOG` from `sandbox.logLevel`. See `docs/logging.md`.

The chart always sets the three name keys from the release's `fullname`; the fallbacks only matter for hand-written configs.

## Behavior Reference

- **Cluster targets.** The egress allowlist (`cluster-service` rows that apply to the run: instance-scoped plus the run's project) is resolved against the cluster for every provision and reconcile, rule by rule. A rule that cannot be resolved never blocks the others:
  - **Transient errors** (timeout, `5xx`, `408`, `429`, connection errors, a named port without a ready endpoint): the rule's last good resolution is used, with a warning naming the rule, so a hiccup does not revoke access of a live run.
  - **Permanent errors** (`404`, `403` and other `4xx`; a headless Service without `allowDns`; configured ports that match no Service port; no ports at all): the cached resolution is evicted and the rule is skipped with a warning naming it, so stale access (old selectors, `/32` blocks) is not kept.
  - Without a cached resolution a failing rule is skipped (fail closed).
  - A rule that is no longer in the allowlist is dropped from every policy. The cache is kept per project key and pruned to the rules of the latest call of that project, so a removed and re-added rule starts without history.
  - Provisioning fails only if the allowlist itself cannot be read (database error). One bad entry does not fail provisioning of other runs; the affected run starts without that target.
  - Rules resolve one after another (worst case two API calls of `K8S_API_TIMEOUT` per rule).
- **Named `targetPort`.** Resolved through the EndpointSlices of the Service. EndpointSlice ports are named after the Service port's `name`, so that name is matched; ports of all matching slices are allowed (pods may use different numbers for the same name).
- **Sandbox events.** `Started` once a pod runs with an IP. `Gone` once per run for a deleted or failed pod, an image-pull / config error in a container (even if the pod never ran), a deleted Sandbox, or a pod / Sandbox that is missing after a watch relist (a relist never sends deletes). Only objects this process has seen are compared; after a control plane restart nothing is known, and the scheduler does **not** resync active runs against the cluster: a run whose pod vanished while the control plane was down ends through the scheduler's connect timeout or wall-clock hard stop.
- **Timeouts.** Every Kubernetes API call made by this crate is bounded by `K8S_API_TIMEOUT` (5 s).
- **Loops.** The reconcile loop ends when shutdown is requested, or when the shutdown or leader channel sender is gone. A closed notification channel only disables that trigger; the 30 s resync continues.

## Leadership & NetworkPolicy Reconciliation

Reconciliation of sandbox `NetworkPolicy` objects is managed by the leader instance:
- **Leader Election Lease**: Stored as a `coordination.k8s.io/v1` `Lease` named `<instanceName>-leader` in the instance namespace.
  - Duration: 15 seconds (`DEFAULT_LEASE_DURATION_SECONDS`, must be positive), renewed every 5 seconds.
  - Shutdown: The lease holder identity and renew timestamp are cleared upon graceful shutdown so standby instances can acquire leadership immediately without waiting for lease expiration.
- **Reconciliation Triggers**:
  1. *Database Notifications*: Listens on the PostgreSQL notification channel for `egress_policy` and `reconnected` events, triggering immediate policy re-rendering.
  2. *Kubernetes Watches*: Watches `Service` and `EndpointSlice` objects in the cluster-target namespaces (`clusterTargets.namespaces`, all namespaces with `anyNamespace`), with backoff; watch errors are logged. With no target namespace nothing is watched.
  3. *Debounce*: notifications arriving within 1 s of the first one are served by one reconcile pass.
  4. *Periodic Resync*: Re-syncs every 30 seconds to reconcile drift or out-of-band policy edits.
- **TLS Name Derivation**: See `sandbox.controlPlaneTlsServerName`.

## Required RBAC Permissions

The control plane verifies the following with `SelfSubjectAccessReview` at startup and refuses to start if one is missing; the error lists every missing permission of every checked namespace. The chart grants them (`templates/rbac.yaml`).
- **Sandbox Namespace**:
  - `agents.x-k8s.io` `sandboxes`: `create`, `get`, `delete`, `list`, `watch`, `patch`
  - `core` `persistentvolumeclaims`: `create`, `get`, `list`, `watch`, `patch`, `delete`
  - `networking.k8s.io` `networkpolicies`: `create`, `get`, `delete`, `list`, `watch`, `patch`
  - `core` `pods`: `get`, `list`, `watch`
- **Instance Namespace**:
  - `coordination.k8s.io` `leases`: `create`, `get`, `update`
  - `core` `services`: `get`
  - `core` `secrets`: `get`
- **Each cluster-target namespace** (cluster-wide with `anyNamespace`):
  - `core` `services`: `get`, `list`, `watch`
  - `discovery.k8s.io` `endpointslices`: `list`, `watch` (the chart grants `get` as well)
- **Cluster Scope**:
  - `core` `namespaces`: `get` (scoped to the sandbox namespace via `resourceNames`). A `403` on this lookup is reported as a missing permission.

## Test Coverage

Line coverage from `cargo llvm-cov -p df-k8s`: **86.88 %** (3,141 lines, 412 missed; the figure includes the inline test code of each file). Region coverage 85.81 %. The target is 70 %.

Not covered by automated runs here (`#[ignore]`, need the dev cluster; the owner reports having run the dev-cluster checks, but the test code itself was not re-run here): `test_provider_live_cluster_integration`, `test_reconciliation_live_cluster` (calls `reconcile_active_runs` directly; it does not exercise the loop or the 30 s window), and in `dark-factory` `serve_startup_check_refuses_to_start_with_missing_role` and `serve_starts_with_the_charts_rbac`.
