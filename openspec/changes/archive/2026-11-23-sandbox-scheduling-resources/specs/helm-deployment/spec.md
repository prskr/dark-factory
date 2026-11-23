# Spec Delta

## MODIFIED Requirements

### Requirement: Sandbox namespace
With `sandboxNamespace.create: true` the chart SHALL create the sandbox namespace with Pod Security Admission `restricted`, a default-deny ingress and egress NetworkPolicy and a ResourceQuota; otherwise it SHALL reference an existing namespace. The sandbox namespace SHALL contain no Secrets other than the image pull secrets an operator places there for `sandbox.imagePullSecrets`.

#### Scenario: Default-deny baseline
- **WHEN** a pod without a run policy starts in the sandbox namespace
- **THEN** it can neither send nor receive any traffic

### Requirement: No Secret access in the sandbox namespace
The chart SHALL grant the control plane no access to Secrets in the sandbox namespace.

#### Scenario: No Secret access with image pull secrets
- **WHEN** the chart is rendered with `sandbox.imagePullSecrets` set
- **THEN** the control plane's sandbox Role grants no verb on `secrets`

### Requirement: Sandbox image pull secret values
The chart SHALL offer `sandbox.imagePullSecrets` (default `[]`), a list of references `{ name: "<Secret>" }` to existing Secrets in the sandbox namespace, and render their names as `sandbox.imagePullSecrets` of the instance configuration. The chart SHALL NOT create these Secrets and SHALL NOT fall back to `image.pullSecrets`. The values schema SHALL reject an entry without a non-empty `name`.

#### Scenario: Pull secrets set
- **WHEN** `sandbox.imagePullSecrets` is `[{ name: corp-mirror }]`
- **THEN** the instance configuration has `sandbox.imagePullSecrets: ["corp-mirror"]` and the chart renders no Secret named `corp-mirror`

#### Scenario: Default
- **WHEN** the chart is rendered with default values
- **THEN** the instance configuration has no `sandbox.imagePullSecrets` entries

#### Scenario: Independent of the instance pull secrets
- **WHEN** `image.pullSecrets` is `[{ name: release-registry }]` and `sandbox.imagePullSecrets` is empty
- **THEN** the instance configuration has no `sandbox.imagePullSecrets` entries

#### Scenario: Entry without a name
- **WHEN** `sandbox.imagePullSecrets` is `[{ name: "" }]`
- **THEN** the render fails schema validation

## ADDED Requirements

### Requirement: Sandbox pod values
The chart SHALL take every setting of the sandbox pod from `sandbox.*` and every setting of the sandbox namespace and its objects from `sandboxNamespace.*`. `sandbox.runtimeClassName` (default `""`) and `sandbox.serviceAccountName` (default `dark-factory-sandbox`) SHALL be rendered as the instance configuration keys of the same name; the chart SHALL create the ServiceAccount under that name. A render with one of the old keys below `sandboxNamespace` SHALL fail with a message naming the new key.

#### Scenario: Runtime class
- **WHEN** the chart is rendered with `sandbox.runtimeClassName: kata`
- **THEN** the instance configuration has `sandbox.runtimeClassName: kata`

#### Scenario: Service account
- **WHEN** the chart is rendered with `sandboxNamespace.create: true` and `sandbox.serviceAccountName: agents`
- **THEN** the instance configuration has `sandbox.serviceAccountName: agents` and the chart renders a ServiceAccount `agents` in the sandbox namespace

#### Scenario: Old keys rejected
- **WHEN** the chart is rendered with `sandboxNamespace.runtimeClassName: kata`
- **THEN** rendering fails with a message that `sandboxNamespace.runtimeClassName` moved to `sandbox.runtimeClassName`; the same holds for `imagePullSecrets` and `serviceAccountName`

### Requirement: Sandbox scheduling values
The chart SHALL offer `sandbox.nodeSelector` (map of label key to value, default `{}`), `sandbox.tolerations` (list of Kubernetes tolerations, default `[]`) and `sandbox.affinity` (Kubernetes affinity, default `{}`) and render them under the same keys in the instance configuration. The values schema SHALL reject a non-string node selector value, a toleration with an unknown field, operator or effect, and an affinity key other than `nodeAffinity`, `podAffinity` and `podAntiAffinity`.

#### Scenario: Dedicated sandbox nodes
- **WHEN** the chart is rendered with `sandbox.nodeSelector: {dark-factory.io/pool: sandbox}` and `sandbox.tolerations: [{key: dark-factory.io/sandbox, operator: Exists, effect: NoSchedule}]`
- **THEN** the instance configuration has the same `sandbox.nodeSelector` and `sandbox.tolerations`

#### Scenario: Soft spread of sandboxes
- **WHEN** the chart is rendered with a `sandbox.affinity` that has a preferred pod anti-affinity on `kubernetes.io/hostname`
- **THEN** the instance configuration has the same `sandbox.affinity`

#### Scenario: Default
- **WHEN** the chart is rendered with default values
- **THEN** the instance configuration has no `sandbox.nodeSelector` entries, no `sandbox.tolerations` entries and no `sandbox.affinity`

#### Scenario: Unknown affinity key rejected
- **WHEN** the chart is rendered with `sandbox.affinity: {antiAffinity: {}}`
- **THEN** rendering fails schema validation

#### Scenario: Unknown operator rejected
- **WHEN** the chart is rendered with a `sandbox.tolerations` entry with operator `In`
- **THEN** rendering fails schema validation

#### Scenario: Non-string label value rejected
- **WHEN** the chart is rendered with `sandbox.nodeSelector: {dedicated: true}`
- **THEN** rendering fails schema validation

### Requirement: Sandbox resource values
The chart SHALL offer `sandbox.resources` with `requests` and `limits` (maps of resource name to quantity) with defaults in `values.yaml`, and render both maps as `sandbox.resources` of the instance configuration, a missing map as empty and every quantity as a string. Overrides follow Helm's value merging. The values schema SHALL reject an unknown key below `sandbox.resources` and a quantity that is neither a string nor a number.

#### Scenario: Default
- **WHEN** the chart is rendered with default values
- **THEN** the instance configuration has `sandbox.resources` with requests `cpu: 250m`, `memory: 512Mi` and limits `cpu: "2"`, `memory: 4Gi`

#### Scenario: Override merges per key
- **WHEN** the chart is rendered with `sandbox.resources.limits.memory: 8Gi`
- **THEN** the instance configuration has `sandbox.resources.limits` `{cpu: "2", memory: 8Gi}` and the default requests

#### Scenario: Entry removed with null
- **WHEN** the chart is rendered with `sandbox.resources.limits.cpu: null` and `sandboxNamespace.create: false`
- **THEN** the instance configuration has `sandbox.resources.limits` `{memory: 4Gi}` without a `cpu` entry

#### Scenario: Map removed with null
- **WHEN** the chart is rendered with `sandbox.resources: {requests: null, limits: null}` and `sandboxNamespace.create: false`
- **THEN** the instance configuration has `sandbox.resources` with empty `requests` and empty `limits`, and sandbox pods get no resources from Dark Factory

#### Scenario: Numeric quantity accepted
- **WHEN** the chart is rendered with `sandbox.resources.limits.cpu: 2` (a number)
- **THEN** the instance configuration has `sandbox.resources.limits.cpu: "2"` (a string)

#### Scenario: Other quantity types rejected
- **WHEN** the chart is rendered with `sandbox.resources.limits.cpu: true`
- **THEN** rendering fails schema validation

### Requirement: Sandbox resources fit the quota
When the chart renders the sandbox namespace (`sandboxNamespace.create: true`) the render SHALL fail unless `sandbox.resources` sets requests and limits for both `cpu` and `memory`, with a message naming the missing keys and the `ResourceQuota`.

#### Scenario: Memory limit removed
- **WHEN** the chart is rendered with `sandboxNamespace.create: true` and `sandbox.resources.limits.memory: null`
- **THEN** rendering fails with a message naming `sandbox.resources.limits.memory`

#### Scenario: Defaults pass
- **WHEN** the chart is rendered with `sandboxNamespace.create: true` and default values
- **THEN** rendering succeeds

#### Scenario: Existing namespace
- **WHEN** the chart is rendered with `sandboxNamespace.create: false` and `sandbox.resources: {requests: null, limits: null}`
- **THEN** rendering succeeds

### Requirement: Control plane and gateway anti-affinity
The chart SHALL give the control plane and the gateway pods a pod anti-affinity against the pods of the same component, release and revision (`pod-template-hash`) on the same node (`kubernetes.io/hostname`), selected by `controlPlane.podAntiAffinity` and `gateway.podAntiAffinity`: `soft` (default, preferred with weight 100), `hard` (required) or `none` (no anti-affinity). The values schema SHALL reject any other value.

#### Scenario: Default soft rule
- **WHEN** the chart is rendered with default values
- **THEN** the control plane and the gateway Deployments each have a preferred pod anti-affinity with topology key `kubernetes.io/hostname` that selects only their own component's pods of the release

#### Scenario: Single-node cluster
- **WHEN** a release with the default values and `gateway.replicas: 2` is installed on a cluster with one node
- **THEN** both gateway pods are scheduled on that node

#### Scenario: Hard rule
- **WHEN** the chart is rendered with `gateway.podAntiAffinity: hard`
- **THEN** the gateway Deployment has a required pod anti-affinity with topology key `kubernetes.io/hostname` and `matchLabelKeys: [pod-template-hash]`

#### Scenario: Hard rule does not block a rollout
- **WHEN** `gateway.podAntiAffinity` is `hard`, `gateway.replicas` is 2, the cluster has 2 nodes with room for one more gateway pod each, and the gateway pod template changes
- **THEN** the rollout completes, because the new pods do not count the old revision's pods; an old and a new gateway pod may share a node until the old one ends

#### Scenario: No rule
- **WHEN** the chart is rendered with `controlPlane.podAntiAffinity: none`
- **THEN** the control plane Deployment has no `affinity`

#### Scenario: Invalid preset rejected
- **WHEN** the chart is rendered with `controlPlane.podAntiAffinity: strict`
- **THEN** rendering fails schema validation

### Requirement: Control plane and gateway affinity override
The chart SHALL offer `controlPlane.affinity` and `gateway.affinity` (default `{}`). A non-empty value SHALL be rendered as the pod's `affinity` as is and replace the anti-affinity preset of that component.

#### Scenario: Zone spreading
- **WHEN** the chart is rendered with a `gateway.affinity` that has a pod anti-affinity with topology key `topology.kubernetes.io/zone`
- **THEN** the gateway Deployment has exactly that `affinity` and no `kubernetes.io/hostname` rule

#### Scenario: Empty override keeps the preset
- **WHEN** the chart is rendered with `controlPlane.affinity: {}`
- **THEN** the control plane Deployment has the anti-affinity of `controlPlane.podAntiAffinity`

### Requirement: No sandbox LimitRange
The chart SHALL NOT create a LimitRange in the sandbox namespace: the size of sandbox pods comes only from `sandbox.resources`. A render with `sandboxNamespace.limitRange` SHALL fail with a message pointing to `sandbox.resources`.

#### Scenario: No LimitRange
- **WHEN** the chart is rendered with `sandboxNamespace.create: true`
- **THEN** it renders a ResourceQuota and no LimitRange in the sandbox namespace

#### Scenario: Removed limitRange value
- **WHEN** the chart is rendered with `sandboxNamespace.limitRange.defaultCpu: "2"`
- **THEN** rendering fails with a message naming `sandbox.resources`
