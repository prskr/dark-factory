## MODIFIED Requirements

### Requirement: Project settings tab in the URL
The open tab of the project settings page SHALL be part of the URL as `tab=security`, `tab=egress`, `tab=sidecars` or `tab=integrations`, and left out for General. Opening such a URL SHALL show that tab, choosing a tab SHALL update the URL without a page load, and an unknown or non-permitted value SHALL show General.

#### Scenario: Link to a tab
- **WHEN** an admin opens `/projects/DF?tab=integrations`
- **THEN** the page opens with the Integrations tab open

#### Scenario: Link to the Egress tab
- **WHEN** a board member opens `/projects/DF?tab=egress`
- **THEN** the page opens with the Egress tab open

#### Scenario: Tab not permitted
- **WHEN** a user with only `board-member` opens `/projects/DF?tab=integrations`
- **THEN** the page opens with the General tab open and shows no Integrations tab

#### Scenario: Security tab content
- **WHEN** a board member opens the Security tab of `DF`
- **THEN** it shows the push checks and the hidden paths of `DF`

#### Scenario: Choosing a tab updates the URL
- **WHEN** a board member on `/projects/DF` chooses Security
- **THEN** the URL becomes `/projects/DF?tab=security` without a page load

#### Scenario: Link to the Sidecars tab
- **WHEN** a board member opens `/projects/DF?tab=sidecars`
- **THEN** the page opens with the Sidecars tab open

#### Scenario: Old tab value
- **WHEN** a board member opens `/projects/DF?tab=push-checks`
- **THEN** the page opens with the General tab open


## ADDED Requirements

### Requirement: Sidecars tab
The project settings page SHALL have a Sidecars tab after Egress, without the "Save project" button. It SHALL list the project's sidecars in their order with name, image, ports and markers for MCP endpoint and egress `proxy`, and SHALL show the sums of their CPU and memory requests and limits. Add, edit, reorder and delete SHALL save each sidecar on its own, through a form modal for add and edit. Without sidecars the tab SHALL show an empty state with an "Add sidecar" action.

#### Scenario: Two sidecars
- **WHEN** a board member opens the Sidecars tab of `DF`, which has sidecars `playwright` and `postgres`
- **THEN** both are listed with image and ports, `playwright` is marked as MCP server, the sums of their requests and limits are shown, and no "Save project" button

#### Scenario: Empty state
- **WHEN** a board member opens the Sidecars tab of a project without sidecars
- **THEN** an empty state explains sidecars and offers "Add sidecar"

#### Scenario: Reorder
- **WHEN** a board member moves `postgres` before `playwright`
- **THEN** the order is stored and later runs start `postgres` first
