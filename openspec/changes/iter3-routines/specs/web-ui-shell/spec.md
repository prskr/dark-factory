# Spec Delta

## MODIFIED Requirements

### Requirement: Desktop navigation rail
At widths of 768 px and more, every authenticated page SHALL show a navigation rail with the logo, a new-task action, links to Tasks, Requests, Routines and Network traffic, and at its bottom a Settings link above the signed-in board member's avatar that opens the profile menu. The links SHALL show icons only; each section's name SHALL be its accessible name and its tooltip. The current section SHALL be marked visually with a bar at the rail's edge and with `aria-current`.

#### Scenario: Rail on the task page
- **WHEN** a board member opens `/tasks` at 1440 px width
- **THEN** the rail shows the new-task action, Tasks marked as current, Requests, Routines, Network traffic, Settings and the avatar, and no links to Projects, Personas, Credentials, Model providers or Egress rules and no separate log-out button

#### Scenario: Settings current on a settings page
- **WHEN** a board member opens `/credentials` at 1440 px width
- **THEN** the rail marks Settings as current

#### Scenario: Settings current on the projects page
- **WHEN** a board member opens `/projects` at 1440 px width
- **THEN** the rail marks Settings as current

#### Scenario: Rail on the routines page
- **WHEN** a board member opens `/routines` at 1440 px width
- **THEN** the rail shows Routines below Requests, marked as current

#### Scenario: Section names as tooltips
- **WHEN** the rail is rendered
- **THEN** no section name is shown as visible text, and each section link has its name as accessible name and title

#### Scenario: Rail in a split-screen window
- **WHEN** a board member opens `/tasks` at 900 px width
- **THEN** the rail is shown and no bottom bar

#### Scenario: Log out from the rail
- **WHEN** a board member at 1440 px width activates the avatar on the rail and chooses Log out
- **THEN** the session ends and the browser leaves the UI
