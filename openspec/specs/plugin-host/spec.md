# plugin-host Specification

## Purpose

Executes integration plugins (WebAssembly components implementing `darkfactory:integrations@0.1.0`) so that HTTP through the host is their only way out and they never see credentials (ADR-0002, ADR-0028).

## Requirements

### Requirement: No ambient capabilities
Plugins SHALL run with standard WASI linked but no preopened directories, no environment variables, no sockets and no inherited stdin/stdout; stderr output SHALL be forwarded to the host's logs tagged with the plugin instance.

#### Scenario: Plugin reads the environment
- **WHEN** a plugin lists its environment variables
- **THEN** the list is empty

### Requirement: Outgoing HTTP restricted to allowed hosts
The host SHALL allow outgoing requests only to the host of the plugin instance's API base URL (the route's `apiUrl`, by default scheme and host of the project's repository URL), over HTTPS, and SHALL fail other requests with a transport error before any connection is made.

#### Scenario: Request to another host
- **WHEN** the Forgejo plugin for `git.example.org` requests `https://evil.example.com/`
- **THEN** the request fails with a transport error and no connection is opened

#### Scenario: Separate API host
- **WHEN** the route for `github.com` sets `apiUrl: https://api.github.com` and the plugin requests `https://api.github.com/repos/b/y/pulls`
- **THEN** the request is allowed, and a request to `https://github.com/` fails with a transport error

### Requirement: Credentials injected by the host
The host SHALL set the `Authorization` header from the instance's credential on every allowed request, in the plugin's configured scheme (`authScheme`: `token` → `token <credential>`, the default; `bearer` → `Bearer <credential>`), replacing any `Authorization` header set by the plugin. The plugin SHALL never receive the credential value.

#### Scenario: Plugin sets its own header
- **WHEN** a plugin sends `Authorization: Bearer fake`
- **THEN** the upstream receives the host-injected credential instead

#### Scenario: Bearer scheme
- **WHEN** `plugins.github.authScheme` is `bearer`
- **THEN** the upstream receives `Authorization: Bearer <credential>`

#### Scenario: Unknown scheme
- **WHEN** `authScheme` is `basic`
- **THEN** `serve` exits at startup with an error naming the plugin and the value

### Requirement: Non-secret configuration
Each plugin instance SHALL receive its non-secret settings (at least `base-url`) through `wasi:config/store`.

#### Scenario: Base URL
- **WHEN** the plugin reads `base-url` and the project's route sets no `apiUrl`
- **THEN** it gets the scheme and host of the project's repository URL

#### Scenario: Base URL from the route
- **WHEN** the plugin reads `base-url` and the route for the project's host sets `apiUrl: https://api.github.com`
- **THEN** it gets `https://api.github.com`

### Requirement: Resource limits
Each plugin call SHALL be bounded by a configurable timeout (default 30 s) and memory limit (default 64 MiB); exceeding either SHALL fail the call with an internal error without affecting other calls.

#### Scenario: Plugin loops forever
- **WHEN** a plugin call doesn't return within the timeout
- **THEN** the call fails, the instance is discarded and the next call uses a fresh instance

### Requirement: Load from a file path
The host SHALL load each plugin component from its configured file path at startup and SHALL NOT download, unpack or cache components itself. A missing or invalid component SHALL be logged with a message naming the plugin and the path and skipped; startup SHALL fail only if no forge plugin (one exporting `pull-requests`) could be loaded. Loading SHALL log the component size and the load duration (everything after the file read).

#### Scenario: Missing component
- **WHEN** `plugins.forgejo.path` points to a file that does not exist and `forgejo` is the only plugin
- **THEN** `serve` exits at startup with an error naming `forgejo` and the path

#### Scenario: One of two plugins missing
- **WHEN** `plugins.github.path` does not exist and `plugins.forgejo` loads
- **THEN** `serve` starts, an error event names `github` and the path, and projects on hosts `github` claims fail as described in `forge-routing`

#### Scenario: Load is measured
- **WHEN** the Forgejo component loads
- **THEN** an info event carries the plugin name, the size in bytes and the load duration (`compile_ms`, everything after the file read)

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
