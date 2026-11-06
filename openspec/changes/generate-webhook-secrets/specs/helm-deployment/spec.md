# Spec Delta

## MODIFIED Requirements

### Requirement: Least-privilege RBAC
The chart SHALL grant exactly the permissions in `design/deployment.md` §3: the control plane's write access only in the sandbox namespace, Secret read and Leases in the instance namespace, Secret create in the instance namespace only while at least one plugin uses a generated webhook secret, Service/EndpointSlice read in cluster-target namespaces; no Kubernetes API access for the gateway and sandboxes. `pods/exec`, `pods/attach` and namespace create/delete SHALL never be granted.

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

### Requirement: Plugin webhook secrets
`plugins.<name>.webhookSecret` SHALL accept `{existingSecret, key}`, both optional. The chart SHALL render `plugins.<name>.webhookSecret` in the instance config as a reference to a Secret in the release namespace: the existing Secret, read only, when `existingSecret` is set; otherwise `<fullname>-webhook-<name>` with generation allowed. `key` defaults to `secret`. The chart SHALL NOT create or mount webhook Secrets.

#### Scenario: Secret reference
- **WHEN** `plugins.forgejo.webhookSecret` is `{existingSecret: forge-webhooks, key: forgejo}`
- **THEN** the instance config references Secret `forge-webhooks`, key `forgejo`, without generation, and no webhook volume is rendered

#### Scenario: Default values
- **WHEN** the chart is rendered with default values and release name `dark-factory`
- **THEN** the instance config references Secret `dark-factory-webhook-forgejo`, key `secret`, with generation allowed, and the chart renders no Secret for it

#### Scenario: Rendering is stable
- **WHEN** the chart is rendered twice with the same values
- **THEN** both renders are identical
