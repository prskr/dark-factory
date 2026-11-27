## MODIFIED Requirements

### Requirement: Published documentation
The docs SHALL contain an install guide, the logging, telemetry and Keycloak guides, and the configuration references of the sandbox runtime, the agent channel and the plugins. Contributor documentation, ADRs and design documents SHALL NOT be published.

#### Scenario: Install guide
- **WHEN** an operator follows the install guide on a cluster with Kata Containers, an external PostgreSQL and Keycloak
- **THEN** the guide installs the chart from `oci://ghcr.io/prskr/dark-factory/charts/dark-factory` with a pinned `--version` and ends with a working login

#### Scenario: No contributor docs
- **WHEN** the site is built
- **THEN** it contains no page generated from `docs/development.md`, `docs/tech-debt/` or `docs/adr/`
