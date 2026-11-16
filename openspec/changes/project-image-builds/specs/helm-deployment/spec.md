## ADDED Requirements

### Requirement: Build namespace
With `buildNamespace.create: true` the chart SHALL create the build namespace `buildNamespace.name` with Pod Security Admission `baseline`; otherwise it SHALL reference an existing namespace. The chart SHALL NOT create Secrets in it; the operator creates the push Secret there.

#### Scenario: Created namespace
- **WHEN** the chart is rendered with `buildNamespace.create: true` and `name: df-builds`
- **THEN** namespace `df-builds` has the label `pod-security.kubernetes.io/enforce: baseline`

### Requirement: Build network policy
In the build namespace the chart SHALL render a NetworkPolicy for build pods that denies all ingress and allows egress to DNS, to every address outside `buildNamespace.excludedCidrs` (the cluster's pod and service ranges) and outside the link-local ranges `169.254.0.0/16` and `fe80::/10`, and to each entry of `buildNamespace.clusterTargets` (namespace and pod selector with ports, or a CIDR with ports).

#### Scenario: In-cluster registry
- **WHEN** `clusterTargets` lists namespace `registry`, pod label `app=registry`, port 5000
- **THEN** build pods may connect to those pods on port 5000 and to no other in-cluster pod

#### Scenario: Internet egress
- **WHEN** a build pod connects to a public package mirror on port 443
- **THEN** the policy allows it

#### Scenario: Cloud metadata endpoint
- **WHEN** a build step connects to `169.254.169.254`
- **THEN** the policy denies it, also when `excludedCidrs` is empty

### Requirement: Build RBAC
When `imageBuild.enabled` is true the chart SHALL grant the control plane in the build namespace: pods create, get, list, watch and delete; `pods/log` get; configmaps create, get and delete; events get, list and watch. It SHALL NOT grant Secret access, `pods/exec` or `pods/attach` there.

#### Scenario: No Secret access
- **WHEN** `kubectl auth can-i get secrets --as=<control plane SA> -n <build ns>` is evaluated
- **THEN** the answer is `no`

### Requirement: Image build values
The chart SHALL expose `imageBuild.enabled` (default `false`), `registryPrefix`, `pushSecret`, `builder.{image,args}`, `pusher.{image,args}`, `deadlineSeconds` (default 1800), `maxConcurrent` (optional), `resources`, and `sandbox.defaultImage` (optional), and render them into the instance configuration, with `imageBuild.namespace` taken from `buildNamespace.name`. With `imageBuild.enabled: true`, the values schema SHALL require `buildNamespace.name`, `registryPrefix` and `pushSecret`.

#### Scenario: Missing registry prefix
- **WHEN** the chart is rendered with `imageBuild.enabled: true` and no `registryPrefix`
- **THEN** rendering fails naming `imageBuild.registryPrefix`

#### Scenario: Namespace in the rendered configuration
- **WHEN** the chart is rendered with `imageBuild.enabled: true` and `buildNamespace.name: df-builds`
- **THEN** the instance configuration has `imageBuild.namespace: df-builds`
