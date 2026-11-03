# Spec Delta

## Purpose

Defines the public Dark Factory website: what it publishes (landing page and user documentation), where it is served, how it looks, and the checks that keep it buildable.

## ADDED Requirements

### Requirement: Site address
The site SHALL be served over HTTPS at `https://www.dark-factory.eu/`. Requests to `dark-factory.eu` SHALL redirect to the same path on `www.dark-factory.eu`.

#### Scenario: Apex domain
- **WHEN** a browser opens `http://dark-factory.eu/docs/`
- **THEN** it is redirected to `https://www.dark-factory.eu/docs/`

### Requirement: Landing page
The page at `/` SHALL state what Dark Factory is, that it is pre-MVP and may change incompatibly, what it does, who it is for, who it is not for (no Kubernetes, no Kata Containers, multi-tenant needs, stability needs), and link to the docs and the source repository. It SHALL be built from `main`.

#### Scenario: Visitor without context
- **WHEN** a visitor opens `https://www.dark-factory.eu/`
- **THEN** the page shows the pre-MVP notice, a section on who should not use Dark Factory, and links to `/docs/` and `https://github.com/prskr/dark-factory`

### Requirement: Published documentation
The docs SHALL contain an install guide, the logging, telemetry and Keycloak guides, and the configuration references of the sandbox runtime, the agent channel and the plugins. Contributor documentation, ADRs and design documents SHALL NOT be published.

#### Scenario: Install guide
- **WHEN** an operator follows the install guide on a cluster with Kata Containers, an external PostgreSQL and Keycloak
- **THEN** the guide installs the chart from `oci://ghcr.io/prskr/dark-factory/charts/dark-factory` with a pinned `--version` and ends with a working login

#### Scenario: No contributor docs
- **WHEN** the site is built
- **THEN** it contains no page generated from `docs/development.md`, `docs/tech_debt.md` or `docs/adr/`

### Requirement: Single source for configuration references
The configuration references SHALL be published from the files in the crates that the crates' documentation tests read, not from copies.

#### Scenario: Reference changes with the crate
- **WHEN** a pull request adds a variable to `crates/factory-agent/docs/configuration.md`
- **THEN** the docs built from that pull request list the variable

### Requirement: Site theme
The site SHALL use the web UI's accent, neutral and text colors and its fonts, served from the site itself. Body text and links SHALL meet a contrast ratio of 4.5:1 in both themes. The theme SHALL follow the system setting until the visitor picks light or dark with the toggle.

#### Scenario: Dark system setting
- **WHEN** a visitor with a dark system setting opens the site for the first time
- **THEN** the dark theme is shown

#### Scenario: Links on light background
- **WHEN** a page with links is shown in the light theme
- **THEN** link text has a contrast of at least 4.5:1 against the page background

### Requirement: Search
Each docs version SHALL offer full-text search over its own pages, without a third-party service.

#### Scenario: Search within a version
- **WHEN** a visitor searches for `OTEL_METRICS_EXPORTER` in `/docs/2026.11.0/`
- **THEN** the results link to pages under `/docs/2026.11.0/`

### Requirement: Docs build check
Every pull request that changes the site or a published source file SHALL build the site. The check SHALL fail on a broken link to another page of the site and on an absolute link to the site's own domain, so that links within the site are relative and checked. Links to static files SHALL pass.

#### Scenario: Broken internal link
- **WHEN** a pull request links `telemetry.md` to a page that does not exist
- **THEN** the docs check fails and names the link

#### Scenario: Absolute link to the site
- **WHEN** a pull request links to `https://www.dark-factory.eu/docs/dev/install/`
- **THEN** the docs check fails and asks for a relative link
