# Spec Delta

## MODIFIED Requirements

### Requirement: Project settings page
Each project SHALL have a settings page at `/projects/{key}`, inside the settings area from 768 px and a full page below. It SHALL show a back link to the project list, the project's name, key and forge icon, the tabs General, Security (push checks and hidden paths), Egress (the project's egress rules, see `egress-allowlist`) and, for `admin`, Integrations, and one "Save project" button for General and Security. The Egress tab SHALL NOT show "Save project": its changes are saved by their own dialogs. The project list's name and edit action SHALL link to it, and an unknown key SHALL answer `404`. The tabs follow the approved PenPot board.

#### Scenario: Open a project from the list
- **WHEN** a board member at 1440 px width chooses the edit action of project `DF` on `/projects`
- **THEN** `/projects/DF` opens as a page with the settings sidebar marking Projects, the header "Dark Factory" with key `DF`, and the General tab open

#### Scenario: Unknown project
- **WHEN** `/projects/NOPE` is opened
- **THEN** the response is `404`

#### Scenario: Page on a phone
- **WHEN** a board member opens `/projects/DF` at 390 px width
- **THEN** the page shows the back link, the header and the tabs without a settings sidebar and without horizontal page scrolling

#### Scenario: Egress tab without save button
- **WHEN** a board member opens the Egress tab of `DF`
- **THEN** the project's egress rules are shown and no "Save project" button

### Requirement: Project settings tab in the URL
The open tab of the project settings page SHALL be part of the URL as `tab=security`, `tab=egress` or `tab=integrations`, and left out for General. Opening such a URL SHALL show that tab, choosing a tab SHALL update the URL without a page load, and an unknown or non-permitted value SHALL show General.

#### Scenario: Link to a tab
- **WHEN** an admin opens `/projects/DF?tab=integrations`
- **THEN** the page opens with the Integrations tab open

#### Scenario: Link to the Egress tab
- **WHEN** a board member opens `/projects/DF?tab=egress`
- **THEN** the page opens with the Egress tab open

#### Scenario: Tab not permitted
- **WHEN** a user with only `board-member` opens `/projects/DF?tab=integrations`
- **THEN** the page opens with the General tab open and shows no Integrations tab

#### Scenario: Choosing a tab updates the URL
- **WHEN** a board member on `/projects/DF` chooses Security
- **THEN** the URL becomes `/projects/DF?tab=security` without a page load

#### Scenario: Old tab value
- **WHEN** a board member opens `/projects/DF?tab=push-checks`
- **THEN** the page opens with the General tab open
