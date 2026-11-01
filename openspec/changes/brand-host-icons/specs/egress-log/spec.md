# Spec Delta

## ADDED Requirements

### Requirement: Provider host icons
A destination that is no project host SHALL show the glyph of a known provider when its host is the provider's domain or a subdomain of it, the provider's letter monogram when the provider has no glyph, the server icon for an IP literal and the generic globe otherwise. Glyphs SHALL be served by the control plane.

#### Scenario: Known providers
- **WHEN** a sandbox connects to a host of `github.com`, `githubusercontent.com`, `anthropic.com`, `npmjs.org`, `golang.org`, `pypi.org`, `pythonhosted.org` or `googleapis.com`
- **THEN** the destination shows the glyph of GitHub, GitHub, Anthropic, npm, Go, Python, Python or Google respectively

#### Scenario: Provider with a glyph
- **WHEN** no project uses `github.com` and a sandbox connects to `api.github.com:443`
- **THEN** the destination shows the GitHub glyph, not the `GH` letters

#### Scenario: Anthropic API
- **WHEN** a sandbox connects to `api.anthropic.com:443`
- **THEN** the destination shows the Anthropic glyph

#### Scenario: Provider without a glyph
- **WHEN** a known provider has no glyph and a sandbox connects to its domain
- **THEN** the destination shows the provider's letter monogram

#### Scenario: Look-alike domain
- **WHEN** a sandbox connects to `evilgithub.com:443`
- **THEN** the destination shows the generic globe

#### Scenario: Glyph in both themes
- **WHEN** the page is shown in the light or the dark theme
- **THEN** every provider glyph has a contrast ratio of at least 3:1 against its tile

## MODIFIED Requirements

### Requirement: Graph node cards
Graph nodes SHALL be cards with an icon tile on the panel: a sandbox node shows the run number (`#2`) and the task key, a destination node its host icon, host and port. A node of an unknown source SHALL show its source IP. The sandbox column, heading included, SHALL sit in a dashed group box, and the shorter column SHALL be centred vertically on the taller one.

#### Scenario: Node content
- **WHEN** the graph shows run 2 of task `DF-7` and destination `github.com:443`, and no project uses `github.com`
- **THEN** the sandbox node is a card with an icon tile, `#2` and `DF-7`, and the destination node shows the GitHub glyph, `github.com` and `443`

#### Scenario: Unknown source
- **WHEN** a connection comes from `10.0.0.9`, which is no active sandbox
- **THEN** its sandbox node shows `10.0.0.9`

#### Scenario: Event without a run number
- **WHEN** an event of a known run carries no run number (an older gateway)
- **THEN** the sandbox node shows the first 8 characters of the run id

### Requirement: Project host icons
A destination whose host is a project's repository host, forge API host or issue tracker host SHALL show that project's icon in the events table, graph nodes and details panel. The repository host shows the project's forge icon, the same as in the project list. The forge API host is the host of the `apiUrl` of the route that serves the project's repository; it shows the forge icon when an integration serves the project. An issue tracker host shows the forge icon when it equals the repository host, otherwise the issue-tracker icon. Matching ignores case, a trailing dot and the port, and does not cover subdomains.

#### Scenario: Self-hosted forge
- **WHEN** the instance runs the forgejo integration, project `DF` with a forge credential has repository `https://git.example.internal/org/df.git`, and a sandbox connects to `git.example.internal:443`
- **THEN** the event row, the graph node and the details panel show the Forgejo icon for `git.example.internal`

#### Scenario: Project without forge credential
- **WHEN** project `WEB` without a forge credential has repository `https://code.example.org/web.git` and a sandbox connects to `code.example.org:443`
- **THEN** the destination shows the generic Git icon

#### Scenario: Issue tracker on its own host
- **WHEN** project `DF` has issue tracker `https://jira.example.com/browse/DF` and a sandbox connects to `jira.example.com:443`
- **THEN** the destination shows the issue-tracker icon

#### Scenario: Issue tracker on the repository host
- **WHEN** project `DF` has repository and issue tracker on `git.example.internal`
- **THEN** a connection to `git.example.internal:443` shows the forge icon, not the issue-tracker icon

#### Scenario: Subdomain of a project host
- **WHEN** project `DF` has repository host `git.example.internal` and a sandbox connects to `api.git.example.internal:443`
- **THEN** the destination does not show the project's icon

#### Scenario: Project host before provider monogram
- **WHEN** the instance runs the github integration with route `github.com` → `apiUrl` `https://api.github.com`, project `DF` with a forge credential has repository `https://github.com/org/df.git`, and a sandbox connects to `github.com:443` and `api.github.com:443`
- **THEN** both destinations show the GitHub forge icon instead of the provider glyph

#### Scenario: Forge API on a separate host
- **WHEN** the github integration routes `ghe.example.com` to `apiUrl` `https://api.ghe.example.com/v3`, project `DF` with a forge credential has repository `https://ghe.example.com/org/df.git`, and a sandbox connects to `api.ghe.example.com:443`
- **THEN** the destination shows the GitHub forge icon, not the globe

#### Scenario: Forge API host without a served project
- **WHEN** the github integration routes `github.com` to `apiUrl` `https://api.github.com`, but no project with a forge credential is served by it
- **THEN** a connection to `api.github.com:443` shows the GitHub provider glyph, not the forge icon

#### Scenario: Project created while the page is open
- **WHEN** a project is created while the network traffic page is open
- **THEN** its hosts show the project icon after the page is reloaded
