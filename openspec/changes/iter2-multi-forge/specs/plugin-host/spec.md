# Spec Delta

## MODIFIED Requirements

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
