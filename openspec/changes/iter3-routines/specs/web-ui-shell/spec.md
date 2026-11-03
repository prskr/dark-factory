# Spec Delta

## MODIFIED Requirements

### Requirement: Desktop navigation rail
At widths of 768 px and more, every authenticated page SHALL show a navigation rail with the logo, a new-task action, links to Tasks, Projects, Routines, Personas and Credentials, and a log-out action. The links SHALL show icons only; each section's name SHALL be its accessible name and its tooltip. The current section SHALL be marked visually with a bar at the rail's edge and with `aria-current`.

#### Scenario: Rail on the task page
- **WHEN** a board member opens `/tasks` at 1440 px width
- **THEN** the rail shows the new-task action, Tasks marked as current, Projects, Routines, Personas, Credentials and log out

#### Scenario: Rail on the routines page
- **WHEN** a board member opens `/routines` at 1440 px width
- **THEN** the rail shows Routines below Projects, marked as current

#### Scenario: Section names as tooltips
- **WHEN** the rail is rendered
- **THEN** no section name is shown as visible text, and each section link has its name as accessible name and title

#### Scenario: Rail in a split-screen window
- **WHEN** a board member opens `/tasks` at 900 px width
- **THEN** the rail is shown and no bottom bar
