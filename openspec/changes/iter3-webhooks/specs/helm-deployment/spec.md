# Spec Delta

## ADDED Requirements

### Requirement: Plugin webhook secrets
`plugins.<name>.webhookSecret` SHALL accept a reference `{name, key}` to a Kubernetes Secret in the release namespace. The chart SHALL mount that key read-only into the control plane pod and SHALL write its path as `plugins.<name>.webhookSecretFile` to the instance config. Without the value, the plugin's webhook endpoint answers `404`.

#### Scenario: Secret reference
- **WHEN** `plugins.forgejo.webhookSecret` is `{name: forge-webhooks, key: forgejo}`
- **THEN** the control plane pod mounts key `forgejo` of Secret `forge-webhooks` read-only and the instance config sets `plugins.forgejo.webhookSecretFile` to the mounted path

#### Scenario: Default values
- **WHEN** the chart is rendered with default values
- **THEN** no plugin has a webhook secret and no extra volume is rendered
