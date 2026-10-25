# Spec Delta

## MODIFIED Requirements

### Requirement: Rules table
Each entry SHALL be a row with its target (host icon and monospace host for an `http-host`; namespace/service with a "cluster service" badge for a `cluster-service`), its port or ports, its scope and a delete button with a trash icon and an accessible name that includes the target. A project's repository or issue tracker host SHALL show the project icon as on the network traffic page ("Project host icons"). Otherwise known providers SHALL show a brand monogram (for example `GH` for `github.com` and any subdomain such as `api.github.com`, but not for `evilgithub.com`), IP literals a server icon, other hosts a generic globe.

#### Scenario: Known host
- **WHEN** an entry for `github.com:443` exists and no project uses `github.com`
- **THEN** its row shows the `GH` monogram, `github.com`, port `443` and the scope

#### Scenario: Project host
- **WHEN** the instance runs the forgejo integration, project `DF` with a forge credential has repository `https://git.example.internal/org/df.git`, and an entry for `git.example.internal:443` exists
- **THEN** its row shows the Forgejo icon

#### Scenario: Wildcard entry over a project host
- **WHEN** project `DF` has repository host `git.example.internal` and an entry for `*.git.example.internal:443` exists
- **THEN** its row does not show the project's icon

#### Scenario: Subdomain of a known provider
- **WHEN** an entry for `api.github.com:443` exists
- **THEN** its row shows the `GH` monogram

#### Scenario: IP literal
- **WHEN** a row or event targets `10.0.0.1:443`
- **THEN** it shows the server icon, not the globe

#### Scenario: Unknown host
- **WHEN** an entry for `ghcr.io:443` exists
- **THEN** its row shows the generic globe icon

#### Scenario: Cluster service
- **WHEN** an entry for `nativelink/scheduler` with ports 8980 exists
- **THEN** its row shows `nativelink/scheduler`, a "cluster service" badge and port `8980`

#### Scenario: Delete
- **WHEN** a board member activates the delete button of the row for `pypi.org:443`
- **THEN** the entry is removed as before, with audit entry and notification
