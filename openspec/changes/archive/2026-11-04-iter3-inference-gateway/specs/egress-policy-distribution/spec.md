## ADDED Requirements

### Requirement: Inference routes in the snapshot
Each sandbox entry of the policy snapshot SHALL carry the run's inference route: chosen wire API, upstream URL per offered wire API, credential reference and scheme, and the provider headers rendered for the run. The snapshot SHALL carry the referenced provider credentials once each. Gateways SHALL keep them in memory only and never log them.

#### Scenario: Run with an inference route
- **WHEN** a gateway receives a snapshot while run R of persona "Linus" is active
- **THEN** R's entry names wire API Anthropic Messages, upstream `https://api.anthropic.com`, scheme `claude-subscription`, and the referenced credential is present in the snapshot

#### Scenario: Provider credential rotated
- **WHEN** the Kubernetes Secret behind a model provider's credential changes
- **THEN** gateways receive the new value with the next snapshot, within one minute

#### Scenario: Unresolvable provider credential
- **WHEN** the Secret of a run's model provider cannot be read
- **THEN** the snapshot carries the route without that credential and the other entries are unaffected
