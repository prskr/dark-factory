# Spec Delta

## ADDED Requirements

### Requirement: Lifecycle event signing key
With `lifecycleEvents.signingKey.existingSecret` empty, the chart SHALL create a cert-manager Certificate from the chart's issuer with an Ed25519 private key and `rotationPolicy: Never`. With it set, the chart SHALL use that Secret instead. The chart SHALL mount the key (`lifecycleEvents.signingKey.key`, default `tls.key`) into the control plane read-only.

#### Scenario: Default key from cert-manager
- **WHEN** the chart is rendered with default values
- **THEN** a Certificate with `privateKey.algorithm: Ed25519` and `rotationPolicy: Never` exists and its Secret's `tls.key` is mounted into the control plane

#### Scenario: Existing secret
- **WHEN** the chart is rendered with `lifecycleEvents.signingKey.existingSecret: df-events-key`
- **THEN** no signing key Certificate is rendered and key `tls.key` of Secret `df-events-key` is mounted

### Requirement: Instance subscriptions values
The chart SHALL expose `lifecycleEvents.subscriptions`, a list of objects with `url` (required) and `events` (list of event types, default empty), and render it into the control plane configuration. The values schema SHALL reject unknown event types and entries without a URL.

#### Scenario: Subscription rendered
- **WHEN** the chart is rendered with one subscription `{url: http://audit.svc/events, events: [sandbox.destroyed]}`
- **THEN** the control plane configuration contains that subscription

#### Scenario: Unknown event type
- **WHEN** a subscription lists `sandbox.started`
- **THEN** rendering fails schema validation

### Requirement: Extra control plane arguments
The chart SHALL expose `controlPlane.extraArgs`, a list of strings (default empty), and append each value quoted to the control plane's `serve` arguments.

#### Scenario: Extra argument rendered
- **WHEN** the chart is rendered with `controlPlane.extraArgs: [--unrestricted-project-subscriptions]`
- **THEN** the control plane container's arguments end with `serve` followed by `--unrestricted-project-subscriptions`

#### Scenario: Default
- **WHEN** the chart is rendered with default values
- **THEN** the control plane container's arguments end with `serve`
