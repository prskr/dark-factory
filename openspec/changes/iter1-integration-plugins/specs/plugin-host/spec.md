# Spec Delta

## Purpose

Executes integration plugins (WebAssembly components implementing `darkfactory:integrations@0.1.0`) so that HTTP through the host is their only way out and they never see credentials (ADR-0002, ADR-0028).

## ADDED Requirements

### Requirement: No ambient capabilities
Plugins SHALL run with standard WASI linked but no preopened directories, no environment variables, no sockets and no inherited stdin/stdout; stderr output SHALL be forwarded to the host's logs tagged with the plugin instance.

#### Scenario: Plugin reads the environment
- **WHEN** a plugin lists its environment variables
- **THEN** the list is empty

### Requirement: Outgoing HTTP restricted to allowed hosts
The host SHALL allow outgoing requests only to the hosts configured for the plugin instance, over HTTPS, and SHALL fail other requests with a transport error before any connection is made.

#### Scenario: Request to another host
- **WHEN** the Forgejo plugin for `git.example.org` requests `https://evil.example.com/`
- **THEN** the request fails with a transport error and no connection is opened

### Requirement: Credentials injected by the host
The host SHALL set the `Authorization` header from the instance's credential on every allowed request, replacing any `Authorization` header set by the plugin. The plugin SHALL never receive the credential value.

#### Scenario: Plugin sets its own header
- **WHEN** a plugin sends `Authorization: Bearer fake`
- **THEN** the upstream receives the host-injected credential instead

### Requirement: Non-secret configuration
Each plugin instance SHALL receive its non-secret settings (at least `base-url`) through `wasi:config/store`.

#### Scenario: Base URL
- **WHEN** the plugin reads `base-url`
- **THEN** it gets the scheme and host of the project's repository URL

### Requirement: Resource limits
Each plugin call SHALL be bounded by a configurable timeout (default 30 s) and memory limit (default 64 MiB); exceeding either SHALL fail the call with an internal error without affecting other calls.

#### Scenario: Plugin loops forever
- **WHEN** a plugin call doesn't return within the timeout
- **THEN** the call fails, the instance is discarded and the next call uses a fresh instance

### Requirement: Capability detection
At load time the host SHALL detect which capability interfaces (`pull-requests`, `issues`) a component exports and SHALL refuse to load components whose imports go beyond world `plugin-base`.

#### Scenario: Forge plugin
- **WHEN** the Forgejo component is loaded
- **THEN** the host reports both capabilities

#### Scenario: Plugin importing filesystem
- **WHEN** a component imports `wasi:filesystem/preopens`
- **THEN** loading fails naming the import

### Requirement: Retries per contract
The host SHALL retry calls failing with `rate-limited` (respecting retry-after up to a cap) or `transport`, at most 3 times, and SHALL NOT retry other errors.

#### Scenario: Rate-limited comment
- **WHEN** `add-comment` returns `rate-limited` with retry-after 2 s
- **THEN** the host waits 2 s and retries
