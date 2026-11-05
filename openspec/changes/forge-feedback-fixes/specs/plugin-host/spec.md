# Spec Delta

## MODIFIED Requirements

### Requirement: Resource limits
Each plugin call SHALL be bounded by a configurable timeout (default 30 s, at most 300 s) and memory limit (default 64 MiB); exceeding either SHALL fail the call with an internal error without affecting other calls. A configured timeout above 300 s SHALL be rejected at startup.

#### Scenario: Plugin loops forever
- **WHEN** a plugin call doesn't return within the timeout
- **THEN** the call fails, the instance is discarded and the next call uses a fresh instance

#### Scenario: Timeout above the maximum
- **WHEN** `plugins.forgejo.callTimeoutSeconds` is 600
- **THEN** the control plane refuses to start and names the key and the maximum of 300

### Requirement: Capability detection
At load time the host SHALL detect which capability interfaces (`pull-requests`, `issues`, `webhooks`) a component exports and SHALL refuse to load components whose imports go beyond world `plugin-base`. World `plugin-base` includes the host interface `webhook-signature`. Every import of world `plugin-base`, including the interfaces it pulls in transitively, SHALL be accepted.

#### Scenario: Forge plugin
- **WHEN** the Forgejo component is loaded
- **THEN** the host reports the capabilities `pull-requests`, `issues` and `webhooks`

#### Scenario: Plugin importing filesystem
- **WHEN** a component imports `wasi:filesystem/preopens`
- **THEN** loading fails naming the import

#### Scenario: Plugin importing the signature check
- **WHEN** a component imports `webhook-signature`
- **THEN** it loads

#### Scenario: Contract gains an import
- **WHEN** world `plugin-base` imports an interface the host does not accept
- **THEN** the host's test suite fails naming that interface
