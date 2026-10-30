# Spec Delta

## MODIFIED Requirements

### Requirement: Forge of a project
The UI SHALL show for each project the forge integration that serves it, by icon and name, and a generic Git icon when none serves it. The serving integration SHALL be the loaded plugin that the routing table assigns to the host of the project's repository URL (`forge-routing`), and only for projects with a forge credential.

#### Scenario: Project served by the forgejo integration
- **WHEN** the routing table sends `git.example.org` to `forgejo` and project `DF` on `https://git.example.org/peter/dark-factory.git` has a forge credential
- **THEN** the row of `DF` shows the Forgejo icon with the name "Forgejo"

#### Scenario: Two forges side by side
- **WHEN** `github` claims `github.com`, `forgejo` is the default plugin, and projects `DF` on `https://git.example.org/peter/dark-factory.git` and `WEB` on `https://github.com/acme/web.git` both have forge credentials
- **THEN** the row of `DF` shows the Forgejo icon and the row of `WEB` shows the GitHub icon

#### Scenario: Project without forge credential
- **WHEN** the routing table sends the host of project `WEB` to `forgejo` and `WEB` has no forge credential
- **THEN** the row of `WEB` shows the generic Git icon

#### Scenario: No forge integration
- **WHEN** no loaded plugin serves the host of a project's repository URL
- **THEN** the project shows the generic Git icon

### Requirement: Forge feedback in the project form
While a board member types the repository URL or the issue tracker URL in the project form, the form SHALL show which forge integration the URL maps to through the routing table, by icon and a one-line hint, and a generic icon with a hint that PR and issue features are unavailable when it maps to none. When the plugin claiming the host did not load, the hint SHALL name that plugin.

#### Scenario: Typing a repository URL
- **WHEN** a board member enters `https://git.example.com/factory/dark-factory.git` on an instance whose default plugin is `forgejo`
- **THEN** the field shows the Forgejo icon and the hint names the forgejo integration

#### Scenario: Repository on a claimed host
- **WHEN** a board member enters `https://github.com/acme/web.git` and `github` claims `github.com`
- **THEN** the field shows the GitHub icon and the hint names the github integration

#### Scenario: Claiming plugin not loaded
- **WHEN** a board member enters `https://github.com/acme/web.git`, and `github` claims `github.com` but failed to load
- **THEN** the field shows the generic Git icon and the hint names `github` as not loaded

#### Scenario: Not an HTTPS repository
- **WHEN** the URL is not a valid HTTPS repository URL
- **THEN** the field shows the generic Git icon and the hint says no forge integration applies
