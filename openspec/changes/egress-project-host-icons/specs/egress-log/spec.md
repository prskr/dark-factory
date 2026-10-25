# Spec Delta

## ADDED Requirements

### Requirement: Project host icons
A destination whose host is a project's repository or issue tracker host SHALL show that project's icon in the events table, graph nodes and details panel. The repository host shows the project's forge icon, the same as in the project list. An issue tracker host shows the forge icon when it equals the repository host, otherwise the issue-tracker icon. Matching ignores case, a trailing dot and the port, and does not cover subdomains.

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
- **WHEN** the instance runs the github integration, project `DF` with a forge credential has repository `https://github.com/org/df.git`, and a sandbox connects to `github.com:443`
- **THEN** the destination shows the GitHub icon instead of the `GH` monogram, and `api.github.com` keeps the `GH` monogram

#### Scenario: Project created while the page is open
- **WHEN** a project is created while the network traffic page is open
- **THEN** its hosts show the project icon after the page is reloaded
