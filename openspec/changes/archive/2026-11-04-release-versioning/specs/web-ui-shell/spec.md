# Spec Delta

## MODIFIED Requirements

### Requirement: Desktop navigation rail
At widths of 768 px and more, every authenticated page SHALL show a navigation rail with the logo, a new-task action, links to Tasks, Projects, Personas and Credentials, and at its bottom the signed-in board member's avatar that opens the profile menu. The links SHALL show icons only; each section's name SHALL be its accessible name and its tooltip. The current section SHALL be marked visually with a bar at the rail's edge and with `aria-current`.

#### Scenario: Rail on the task page
- **WHEN** a board member opens `/tasks` at 1440 px width
- **THEN** the rail shows the new-task action, Tasks marked as current, Projects, Personas, Credentials and the avatar, and no separate log-out button

#### Scenario: Section names as tooltips
- **WHEN** the rail is rendered
- **THEN** no section name is shown as visible text, and each section link has its name as accessible name and title

#### Scenario: Rail in a split-screen window
- **WHEN** a board member opens `/tasks` at 900 px width
- **THEN** the rail is shown and no bottom bar

#### Scenario: Log out from the rail
- **WHEN** a board member at 1440 px width activates the avatar on the rail and chooses Log out
- **THEN** the session ends and the browser leaves the UI

## ADDED Requirements

### Requirement: Version in the profile menu
The profile menu SHALL show `Dark Factory <version>` below its actions in the muted text style, with the version the control plane reports (`dev` for unstamped builds). The menu SHALL be the same on the rail and on the phone.

#### Scenario: Released instance
- **WHEN** a board member opens the profile menu on an instance running release `2026.11.0`
- **THEN** the menu shows `Dark Factory 2026.11.0`

#### Scenario: Dev build
- **WHEN** a board member opens the profile menu on an instance built without a version
- **THEN** the menu shows `Dark Factory dev`
