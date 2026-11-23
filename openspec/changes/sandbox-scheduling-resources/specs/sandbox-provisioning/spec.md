# Spec Delta

## ADDED Requirements

### Requirement: Sandbox node selector
Sandbox pods SHALL carry the configured `sandbox.nodeSelector` labels as their `nodeSelector`. When none are configured (unset or empty), sandbox pods SHALL have no `nodeSelector`.

#### Scenario: Node selector configured
- **WHEN** run 1 of `DF-1` is provisioned with `sandbox.nodeSelector` `{dark-factory.io/pool: sandbox}`
- **THEN** pod `run-df-1-1` has `nodeSelector` `{dark-factory.io/pool: sandbox}`

#### Scenario: No node selector configured
- **WHEN** run 1 of `DF-1` is provisioned and `sandbox.nodeSelector` is unset or empty
- **THEN** pod `run-df-1-1` has no `nodeSelector`

#### Scenario: No matching node
- **WHEN** no node carries the configured labels
- **THEN** the pod stays `Pending` and the run ends `connect_timeout` after `runs.connectTimeoutSeconds`

### Requirement: Sandbox tolerations
Sandbox pods SHALL carry the configured `sandbox.tolerations` (key, operator, value, effect, toleration seconds) in the configured order. When none are configured, sandbox pods SHALL have no `tolerations` beyond those Kubernetes adds itself.

#### Scenario: Toleration configured
- **WHEN** run 1 of `DF-1` is provisioned with `sandbox.tolerations` `[{key: dark-factory.io/sandbox, operator: Exists, effect: NoSchedule}]`
- **THEN** pod `run-df-1-1` tolerates the taint `dark-factory.io/sandbox:NoSchedule`

#### Scenario: No tolerations configured
- **WHEN** run 1 of `DF-1` is provisioned and `sandbox.tolerations` is unset or empty
- **THEN** pod `run-df-1-1` has no `tolerations` from Dark Factory

### Requirement: Sandbox affinity
Sandbox pods SHALL carry the configured `sandbox.affinity` (Kubernetes node affinity, pod affinity and pod anti-affinity) as their `affinity`, unchanged. When none is configured (unset or empty), sandbox pods SHALL have no `affinity`. Affinity applies in addition to the node selector and the tolerations.

#### Scenario: Soft spread over nodes
- **WHEN** run 1 of `DF-1` is provisioned with a `sandbox.affinity` that has a preferred pod anti-affinity with weight 100, topology key `kubernetes.io/hostname` and label selector `dark-factory.io/instance: homelab`
- **THEN** pod `run-df-1-1` has exactly that `affinity`

#### Scenario: Spread falls back on a single node
- **WHEN** that soft spread is configured, the cluster has one node and two runs are active
- **THEN** both sandbox pods run on that node

#### Scenario: No affinity configured
- **WHEN** run 1 of `DF-1` is provisioned and `sandbox.affinity` is unset or empty
- **THEN** pod `run-df-1-1` has no `affinity`

### Requirement: Sandbox resources
The agent container of every sandbox pod SHALL have exactly the `requests` and `limits` of `sandbox.resources`. The control plane SHALL add no defaults of its own: a missing or empty map gives the container no entries of that kind, so a LimitRange of the namespace applies.

#### Scenario: Configured resources
- **WHEN** run 1 of `DF-1` is provisioned with `sandbox.resources` `{requests: {cpu: "1", memory: 2Gi}, limits: {memory: 8Gi}}`
- **THEN** the agent container requests `cpu: 1` and `memory: 2Gi`, is limited to `memory: 8Gi` and has no CPU limit

#### Scenario: No resources configured
- **WHEN** run 1 of `DF-1` is provisioned and `sandbox.resources` is not configured, or has empty `requests` and `limits`
- **THEN** the agent container of pod `run-df-1-1` has no resource requests and no limits

#### Scenario: Room for a namespace LimitRange
- **WHEN** run 1 of `DF-1` is provisioned without `sandbox.resources` in a sandbox namespace with an operator-managed LimitRange
- **THEN** the pod the control plane submits has no `resources` on the agent container, so the LimitRange's defaults apply at admission

#### Scenario: Running sandboxes keep their size
- **WHEN** `sandbox.resources` changes while run 1 of `DF-1` is active
- **THEN** pod `run-df-1-1` keeps the resources it was created with, and the next run of `DF-1` gets the new ones

### Requirement: Valid sandbox scheduling and resource settings
The control plane SHALL refuse to start when `sandbox.nodeSelector` has a key that is not a valid Kubernetes label key or a value that is not a valid label value, when a `sandbox.tolerations` entry is invalid, or when a `sandbox.resources` entry has an invalid resource name or quantity. The error SHALL name the option and the invalid entry.

#### Scenario: Invalid label value
- **WHEN** the instance configuration sets `sandbox.nodeSelector` to `{pool: "sandbox nodes"}`
- **THEN** the control plane exits at startup with an error naming `sandbox.nodeSelector` and `pool`

#### Scenario: Exists with a value
- **WHEN** a `sandbox.tolerations` entry has operator `Exists` and value `true`
- **THEN** the control plane exits at startup with an error naming `sandbox.tolerations`

#### Scenario: Toleration seconds without NoExecute
- **WHEN** a `sandbox.tolerations` entry has `tolerationSeconds: 60` and effect `NoSchedule`
- **THEN** the control plane exits at startup with an error naming `sandbox.tolerations`

#### Scenario: Invalid quantity
- **WHEN** the instance configuration sets `sandbox.resources.limits.memory` to `4 GB`
- **THEN** the control plane exits at startup with an error naming `sandbox.resources.limits` and `memory`

### Requirement: Valid sandbox affinity
The control plane SHALL refuse to start when `sandbox.affinity` does not parse as a Kubernetes affinity, has a field name Kubernetes does not know (also with an empty value), has a preferred term weight outside 1–100, a pod affinity term without a topology key, or a selector operator Kubernetes does not know. The error SHALL name `sandbox.affinity` and the path of the invalid entry. Other affinity errors SHALL surface as `provisioning_failed` with the API server's message.

#### Scenario: Misspelled affinity field
- **WHEN** the instance configuration sets `sandbox.affinity.podAntiAffinity.preferredDuringScheduling` (without `IgnoredDuringExecution`)
- **THEN** the control plane exits at startup with an error naming `sandbox.affinity`

#### Scenario: Misspelled field with an empty value
- **WHEN** the instance configuration sets `sandbox.affinity.podAntiAffinity.preferredDuringScheduling` to `[]` or to `null`
- **THEN** the control plane exits at startup with an error naming `sandbox.affinity`

#### Scenario: Known field set to null
- **WHEN** the instance configuration sets `sandbox.affinity.podAntiAffinity.requiredDuringSchedulingIgnoredDuringExecution: null`
- **THEN** the control plane starts and the field counts as unset

#### Scenario: Weight out of range
- **WHEN** a preferred pod anti-affinity term of `sandbox.affinity` has `weight: 101`
- **THEN** the control plane exits at startup with an error naming `sandbox.affinity` and the weight

#### Scenario: Missing topology key
- **WHEN** a pod anti-affinity term of `sandbox.affinity` has no `topologyKey`
- **THEN** the control plane exits at startup with an error naming `sandbox.affinity` and `topologyKey`

## MODIFIED Requirements

### Requirement: Sandbox events
The provider SHALL report when a run's pod is running with its pod IP, and when a run's pod is deleted, failed or completed, or cannot start its container, including the reason. A pod that cannot start because its runtime or harness image volume cannot be pulled counts as unable to start. For a failed pod without a pod-level reason, the reason SHALL name the agent container's termination reason and exit code.

#### Scenario: Pod started
- **WHEN** the run's pod becomes Running with IP 10.0.3.7
- **THEN** a `Started` event with the run id and 10.0.3.7 is emitted

#### Scenario: Image pull fails
- **WHEN** the project image can't be pulled
- **THEN** a `Gone` event with a reason naming the image pull failure is emitted

#### Scenario: Harness image cannot be pulled
- **WHEN** the harness image `registry.example.org/claude-harness:missing` of a run's pod can't be pulled
- **THEN** a `Gone` event with a reason naming the image pull failure and the image is emitted within 30 seconds of the kubelet reporting the failure, and the run doesn't wait for the connect timeout

#### Scenario: Runtime image cannot be pulled
- **WHEN** the runtime image of a run's pod can't be pulled
- **THEN** a `Gone` event with a reason naming the image pull failure is emitted

#### Scenario: Container exits without a report
- **WHEN** the agent container of a run exits with code 0 and the pod reaches phase `Succeeded`
- **THEN** a `Gone` event with a reason naming the completed pod is emitted

#### Scenario: Container killed for memory
- **WHEN** the agent container of a run reaches its memory limit, is terminated with reason `OOMKilled` and exit code 137, and the pod reaches phase `Failed`
- **THEN** a `Gone` event with the reason `OOMKilled (exit code 137)` is emitted and the run ends `sandbox_lost`

#### Scenario: Pod evicted
- **WHEN** the kubelet evicts a run's pod under node memory pressure and the pod has reason `Evicted`
- **THEN** a `Gone` event with a reason naming `Evicted` is emitted
