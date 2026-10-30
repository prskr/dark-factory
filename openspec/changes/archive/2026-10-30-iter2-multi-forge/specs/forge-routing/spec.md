# Spec Delta

## Purpose

Decides which forge plugin serves a project from the host of its repository URL, so several forge plugins can run side by side without wiring projects to plugins.

## ADDED Requirements

### Requirement: Route by repository host
The control plane SHALL send every forge call of a project to the loaded plugin whose `hosts` contains the host (with port, if not the scheme's default) of the project's repository URL, compared case-insensitively. Projects SHALL NOT store a plugin reference.

#### Scenario: Two forges side by side
- **WHEN** `plugins.forgejo.hosts` is `[git.example.org]`, `plugins.github.hosts` is `[github.com]`, project `A` has repository `https://git.example.org/a/x.git` and project `B` has `https://github.com/b/y.git`
- **THEN** pull request calls of `A` go to `forgejo` and those of `B` go to `github`

#### Scenario: Plugin swap
- **WHEN** the operator replaces the plugin claiming `git.example.org` with another plugin claiming the same host and restarts
- **THEN** projects on `git.example.org` use the new plugin without any project change

#### Scenario: Port is part of the host
- **WHEN** a plugin claims `git.example.org:3000` and a project's repository is `https://git.example.org/a/x.git`
- **THEN** the plugin does not serve that project

### Requirement: Default plugin
At most one plugin entry without `hosts` SHALL serve every host that no other plugin claims.

#### Scenario: Single plugin without hosts
- **WHEN** only `plugins.forgejo` is configured, without `hosts`
- **THEN** every project goes to `forgejo`, as in iteration 1

#### Scenario: Default next to a claimed host
- **WHEN** `forgejo` has no `hosts`, `github` claims `github.com` and a project lives on `git.example.org`
- **THEN** the project goes to `forgejo`

### Requirement: Unroutable projects fail as configuration errors
A forge call for a project whose host no loaded plugin serves SHALL fail with a `config` error naming the host. A call for a host claimed by a plugin that failed to load SHALL fail with a `config` error naming that plugin and SHALL NOT be sent to the default plugin.

#### Scenario: Unclaimed host without default
- **WHEN** all plugins list `hosts` and a project lives on `gitlab.com`
- **THEN** the call fails with a `config` error naming `gitlab.com`

#### Scenario: Claiming plugin not loaded
- **WHEN** `github` claims `github.com` but its component failed to load, a default plugin exists and a project lives on `github.com`
- **THEN** the call fails with a `config` error naming `github`, and the default plugin receives no call

### Requirement: Consistent routes at startup
`serve` SHALL refuse to start when a host is claimed by more than one plugin entry or when more than one entry has no `hosts`, naming the conflicting entries. It SHALL log the routing table (host → plugin, and the default plugin) at startup.

#### Scenario: Host claimed twice
- **WHEN** `plugins.a.hosts` and `plugins.b.hosts` both contain `git.example.org`
- **THEN** `serve` exits at startup with an error naming `git.example.org`, `a` and `b`

#### Scenario: Two default plugins
- **WHEN** two plugin entries have no `hosts`
- **THEN** `serve` exits at startup with an error naming both entries
