# Spec Delta

## ADDED Requirements

### Requirement: Sandbox image pull secret values
The chart SHALL offer `sandboxNamespace.imagePullSecrets` (default `[]`), a list of references `{ name: "<Secret>" }` to existing Secrets in the sandbox namespace, and render their names as `sandbox.imagePullSecrets` of the instance configuration. The chart SHALL NOT create these Secrets and SHALL NOT fall back to `image.pullSecrets`. The values schema SHALL reject an entry without a non-empty `name`.

#### Scenario: Pull secrets set
- **WHEN** `sandboxNamespace.imagePullSecrets` is `[{ name: corp-mirror }]`
- **THEN** the instance configuration has `sandbox.imagePullSecrets: ["corp-mirror"]` and the chart renders no Secret named `corp-mirror`

#### Scenario: Default
- **WHEN** the chart is rendered with default values
- **THEN** the instance configuration has no `sandbox.imagePullSecrets` entries

#### Scenario: Independent of the instance pull secrets
- **WHEN** `image.pullSecrets` is `[{ name: release-registry }]` and `sandboxNamespace.imagePullSecrets` is empty
- **THEN** the instance configuration has no `sandbox.imagePullSecrets` entries

#### Scenario: Entry without a name
- **WHEN** `sandboxNamespace.imagePullSecrets` is `[{ name: "" }]`
- **THEN** the render fails schema validation

## MODIFIED Requirements

### Requirement: Sandbox namespace
With `sandboxNamespace.create: true` the chart SHALL create the sandbox namespace with Pod Security Admission `restricted`, a default-deny ingress and egress NetworkPolicy, a ResourceQuota and a LimitRange; otherwise it SHALL reference an existing namespace. The sandbox namespace SHALL contain no Secrets other than the image pull secrets an operator places there for `sandboxNamespace.imagePullSecrets`.

#### Scenario: Default-deny baseline
- **WHEN** a pod without a run policy starts in the sandbox namespace
- **THEN** it can neither send nor receive any traffic

### Requirement: Least-privilege RBAC
The chart SHALL grant exactly the permissions in `design/deployment.md` §3: the control plane's write access to sandbox resources only in the sandbox namespace, Secret read and Leases in the instance namespace, Secret create there only while a plugin uses a generated webhook secret, Service/EndpointSlice read in cluster-target namespaces; no Kubernetes API access for gateway and sandboxes. `pods/exec`, `pods/attach` and namespace create/delete SHALL never be granted. The control plane SHALL have no access to Secrets in the sandbox namespace.

#### Scenario: No exec rights
- **WHEN** `kubectl auth can-i create pods/exec --as=<control plane SA> -n <sandbox ns>` is evaluated
- **THEN** the answer is `no`

#### Scenario: Gateway has no API token
- **WHEN** a gateway pod is inspected
- **THEN** `automountServiceAccountToken` is `false`

#### Scenario: Secret create for generated webhook secrets
- **WHEN** the chart is rendered with default values
- **THEN** the control plane's instance Role allows `create` on `secrets`, and allows neither `update`, `patch` nor `delete` on them

#### Scenario: No Secret create when every secret is provided
- **WHEN** every plugin sets `webhookSecret.existingSecret`
- **THEN** the control plane's instance Role allows only `get` on `secrets`

#### Scenario: No Secret access in the sandbox namespace
- **WHEN** the chart is rendered with `sandboxNamespace.imagePullSecrets` set
- **THEN** the control plane's sandbox Role grants no verb on `secrets`
