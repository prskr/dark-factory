# Proposal: sandbox-scheduling-resources

## Why

Operators cannot place or size sandbox pods through Helm values. A cluster with dedicated (tainted) nodes for agent work cannot pin sandboxes to them. The only sandbox size comes from the `LimitRange` of the chart's sandbox namespace, so it is lost with `sandboxNamespace.create: false`, and it is not a value that later sandbox profiles can override. The control plane and the gateway have no anti-affinity, so two replicas can land on the same node and fail together.

## What Changes

- **Sandbox node selector** `sandbox.nodeSelector` (Helm value and instance configuration key): applied to every sandbox pod.
- **Sandbox tolerations** `sandbox.tolerations`: applied to every sandbox pod. Together with the node selector this pins sandboxes to dedicated nodes.
- **Sandbox affinity** `sandbox.affinity` (optional, default none): a Kubernetes `affinity` (node affinity, pod affinity, pod anti-affinity) applied to every sandbox pod as is, for example a soft anti-affinity that spreads sandboxes over nodes on a best-effort basis.
- **Sandbox resources** `sandbox.resources` (`requests` and `limits`, Kubernetes resource names and quantities): set on the agent container of every sandbox pod. Default: today's effective size (requests `cpu: 250m`, `memory: 512Mi`; limits `cpu: 2`, `memory: 4Gi`), set in the chart's `values.yaml`, so operators see every value; overrides follow Helm's value merging (`null` removes an entry). The control plane adds no defaults: without configured resources the pod gets none, and an operator-managed LimitRange can apply. It is the base that the later sandbox profiles override.
- **BREAKING:** the chart no longer renders the sandbox `LimitRange`, and `sandboxNamespace.limitRange` is removed (a render with it fails with a migration message). The sandbox size lives only in `sandbox.resources`. The `ResourceQuota` stays; when the chart renders it, the render fails if an operator removes a `cpu` or `memory` request or limit from `sandbox.resources` (the defaults set all four), because the quota would reject every sandbox pod.
- **BREAKING:** the Helm values `sandboxNamespace.runtimeClassName`, `sandboxNamespace.imagePullSecrets` and `sandboxNamespace.serviceAccountName` move to `sandbox.runtimeClassName`, `sandbox.imagePullSecrets` and `sandbox.serviceAccountName` (a render with an old key fails with a message naming the new one). Rule: `sandbox.*` is how every sandbox pod looks, `sandboxNamespace.*` is the namespace and its objects. The instance configuration keys do not change; they are already `sandbox.*`.
- **Startup validation** of the new instance configuration keys: invalid label keys or values, invalid tolerations, an affinity with unknown fields and invalid resource quantities stop the control plane at startup with an error naming the option.
- **Out-of-memory reason:** a sandbox pod whose agent container was killed for memory reports `OOMKilled` and the exit code in the `sandbox_lost` detail. Today the detail is only `pod failed`.
- **Default anti-affinity for the control plane and the gateway:** each component prefers to put its replicas on different nodes (soft rule, `kubernetes.io/hostname`). `controlPlane.podAntiAffinity` / `gateway.podAntiAffinity` select `soft` (default), `hard` or `none`; `controlPlane.affinity` / `gateway.affinity` replace the preset with a full Kubernetes `affinity`.
- Operator docs: a how-to for dedicated sandbox nodes and sandbox size, reference entries, troubleshooting rows for `OOMKilled` and pods that stay `Pending`.

Out of scope:
- Sandbox profiles (resources and scheduling per profile, selected per project or per run kind, for example lighter chat runs): roadmap backlog, row "Sandbox pod resources" (reworded at archive to "Sandbox profiles"). This change only fixes the base value they override.
- Node selector and tolerations for the control plane, gateway and migrate pods: roadmap backlog (row added at archive).
- Reporting a sandbox pod that cannot be scheduled (`FailedScheduling`) before the connect timeout: roadmap backlog (row added at archive). Until then it ends `connect_timeout`.
- Gateway rollouts only when a key the gateway reads changes (today every ConfigMap change rolls it): rejected. With two or more gateway replicas a rollout cuts only the connections of the replaced pod; new connections keep working.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `sandbox-provisioning`: sandbox pods get the configured node selector, tolerations, affinity and container resources; the configuration is validated at startup; the `Gone` reason of an out-of-memory kill names `OOMKilled`.
- `helm-deployment`: values and schema for `sandbox.nodeSelector`, `sandbox.tolerations`, `sandbox.affinity`, `sandbox.resources`; `runtimeClassName`, `imagePullSecrets` and `serviceAccountName` move from `sandboxNamespace.*` to `sandbox.*`; the sandbox namespace loses its `LimitRange`; render check against the `ResourceQuota`; default anti-affinity and `affinity` overrides for the control plane and the gateway.

## Impact

- `crates/df-core/src/config.rs`: new fields in `SandboxProvisioningConfig` with defaults and validation.
- `crates/df-k8s/src/render.rs` (`render_pod`, `agent_container`): node selector, tolerations, affinity, resources. The sandbox pod is built here, not in `df-orchestrator`. `crates/df-k8s` startup: affinity check. `crates/df-k8s/src/provider.rs` (`map_pod_event`): container termination reason.
- `deploy/helm/dark-factory`: `values.yaml`, `values.schema.json`, `templates/_helpers.tpl` (`dark-factory.sandboxConfig`), `templates/sandbox-namespace.yaml`, both Deployments, helm-unittest suites, `README.md`; `deploy/values-dev.yaml`, `deploy/values-e2e.yaml`, `deploy/README.md` for the moved keys.
- Docs: `docs/content/operations/` (new page), `troubleshooting.md`, `upgrades.md`, `private-sandbox-images.md`, `getting-started.md`, `architecture/_index.md`, `security/_index.md`, `security/sandbox.md`; `crates/df-k8s/README.md` key table; `design/deployment.md`.
- Upgrade: the control plane and the gateway roll once (new pod template). Running sandboxes keep their old spec; new values apply to pods created afterwards.
- No new dependency, no database or protobuf change.
