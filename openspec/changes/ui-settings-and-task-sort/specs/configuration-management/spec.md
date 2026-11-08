# Spec Delta

## MODIFIED Requirements

### Requirement: Projects
Board members SHALL list, create and edit projects with name, key, repository URL (HTTPS), optional issue tracker URL, sandbox image, optional adapter command, base branch and forge credential reference. The key SHALL be set when the project is created and SHALL be immutable afterwards; the project settings page SHALL show it read-only. The adapter command, if set, replaces the instance's harness image and adapter for this project's runs.

#### Scenario: Create a project
- **WHEN** a board member submits name "Dark Factory", key `DF`, repo `https://git.example.org/peter/dark-factory.git`, image, base `main` and a forge credential
- **THEN** the project is stored and appears in the project list

#### Scenario: Non-HTTPS repository
- **WHEN** the repository URL uses `ssh://` or `http://`
- **THEN** the form shows a validation error and nothing is stored

#### Scenario: Key in the edit form
- **WHEN** a board member opens the settings page of project `DF`, with or without tasks
- **THEN** the key is shown read-only

#### Scenario: Changing the key of a used project
- **WHEN** a board member edits the key of a project that has tasks
- **THEN** the change is rejected with a validation error

#### Scenario: Changing the key of an unused project
- **WHEN** an edit request for project `DF`, which has no tasks, carries a different key
- **THEN** the change is rejected with a validation error and nothing is stored

#### Scenario: Project image with its own adapter
- **WHEN** a board member sets the adapter command `/usr/local/bin/my-acp-adapter` on project `DF`
- **THEN** it is stored and the settings page shows it on the next visit

#### Scenario: Adapter command left empty
- **WHEN** a board member saves a project with an empty adapter command
- **THEN** the project uses the instance's harness image and adapter

### Requirement: Settings area
From 768 px, the pages of projects, personas, credential references, model providers and egress rules SHALL show a settings sidebar between the rail and the page, titled "Settings", with one entry per section (icon and label: Projects, Personas, Credentials, Model providers, Egress rules, in this order). The open section SHALL be marked with a bar and `aria-current`. The rail's Settings link SHALL open the projects page.

#### Scenario: Sidebar on the personas page
- **WHEN** a board member opens `/personas` at 1440 px width
- **THEN** the settings sidebar lists Projects, Personas, Credentials, Model providers and Egress rules with Personas marked as current

#### Scenario: Sidebar on the projects page
- **WHEN** a board member opens `/projects` at 1440 px width
- **THEN** the settings sidebar is shown with Projects marked as current, next to the project table

#### Scenario: Switch section
- **WHEN** a board member chooses Egress rules in the settings sidebar
- **THEN** `/egress/allowlist` opens with the sidebar and Egress rules marked as current

#### Scenario: Settings link
- **WHEN** a board member activates Settings in the rail
- **THEN** `/projects` opens

#### Scenario: No sidebar on phones
- **WHEN** `/personas` or `/projects` is opened at 390 px width
- **THEN** no settings sidebar is shown and the page has no horizontal scrollbar

### Requirement: Form tabs on phones
Below 768 px the tab row of a configuration form or the project settings page SHALL scroll horizontally when its tabs do not fit, without scrolling the page, and SHALL keep the open tab fully visible when the form opens or a tab is chosen.

#### Scenario: Four tabs at 390 px
- **WHEN** an admin opens the settings page of project `DF` at 390 px width and chooses "Integrations", the last of its tabs
- **THEN** the tab row scrolls so that "Integrations" is fully visible, and the page has no horizontal scrollbar

### Requirement: Configuration forms as modals
Creating a project, and creating and editing a persona or credential reference, SHALL happen in a modal over the corresponding list (a full-height sheet below 768 px). Editing a project SHALL happen on the project settings page instead. A validation error SHALL keep the modal open with the entered values and the error at the field. Saving a persona or credential reference SHALL close the modal and update the list without a full page load. Creating a project SHALL close the modal and open the new project's settings page.

#### Scenario: Validation error in the modal
- **WHEN** a credential reference is submitted without a key
- **THEN** the modal stays open with "Key is required." at the Key field and the other values kept

#### Scenario: Saved project
- **WHEN** a board member creates project `WEB` in the "New project" modal
- **THEN** the modal closes and the settings page of `WEB` opens

## ADDED Requirements

### Requirement: Project settings page
Each project SHALL have a settings page at `/projects/{key}`, inside the settings area from 768 px and a full page below. It SHALL show a back link to the project list, the project's name, key and forge icon, the tabs General, Push checks and, for `admin`, Integrations, and one "Save project" button for General and Push checks. The project list's name and edit action SHALL link to it, and an unknown key SHALL answer `404`. The tab orientation follows the approved PenPot board.

#### Scenario: Open a project from the list
- **WHEN** a board member at 1440 px width chooses the edit action of project `DF` on `/projects`
- **THEN** `/projects/DF` opens as a page with the settings sidebar marking Projects, the header "Dark Factory" with key `DF`, and the General tab open

#### Scenario: Unknown project
- **WHEN** `/projects/NOPE` is opened
- **THEN** the response is `404`

#### Scenario: Page on a phone
- **WHEN** a board member opens `/projects/DF` at 390 px width
- **THEN** the page shows the back link, the header and the tabs without a settings sidebar and without horizontal page scrolling

### Requirement: Project settings tab in the URL
The open tab of the project settings page SHALL be part of the URL as `tab=push-checks` or `tab=integrations`, and left out for General. Opening such a URL SHALL show that tab, choosing a tab SHALL update the URL without a page load, and an unknown or non-permitted value SHALL show General.

#### Scenario: Link to a tab
- **WHEN** an admin opens `/projects/DF?tab=integrations`
- **THEN** the page opens with the Integrations tab open

#### Scenario: Tab not permitted
- **WHEN** a user with only `board-member` opens `/projects/DF?tab=integrations`
- **THEN** the page opens with the General tab open and shows no Integrations tab

#### Scenario: Choosing a tab updates the URL
- **WHEN** a board member on `/projects/DF` chooses Push checks
- **THEN** the URL becomes `/projects/DF?tab=push-checks` without a page load

### Requirement: Saving the project settings page
Saving SHALL keep the project settings page open with the stored values and confirm the save visibly and to screen readers. A validation error SHALL keep the entered values and open the tab with the first error.

#### Scenario: Save stays on the page
- **WHEN** a board member changes the base branch of `DF` and saves
- **THEN** the page stays open with the new value, and a "Project saved" status is shown and announced

#### Scenario: Validation error on another tab
- **WHEN** a board member on the Push checks tab saves `DF` with an `ssh://` repository URL entered on General
- **THEN** the page shows the General tab with the error at Repository URL and all entered values kept

### Requirement: Grouped project form
The General tab of the project settings page and the new-project form SHALL show their fields in the groups "Project", "Repository", "Sandbox" and "Forge events", in this order, subject to the approved PenPot board (see the design). Each group SHALL have a heading and SHALL be a region named by it, with a horizontal separator between consecutive groups. Grouping SHALL NOT change field names, validation or what saving stores.

#### Scenario: Groups on the settings page
- **WHEN** a board member opens the settings page of project `DF` on the General tab
- **THEN** "Project" holds Name and Key, "Repository" holds Repository URL, Base branch, Forge credential and Issue tracker URL, "Sandbox" holds Sandbox image and Adapter command, and "Forge events" holds Wake tasks on reviews and mentions, with a separator between consecutive groups and none above the first

#### Scenario: Groups in the new-project form
- **WHEN** a board member opens "New project"
- **THEN** the form shows the same groups, and Key is editable

#### Scenario: Validation error inside a group
- **WHEN** a project is submitted with an `ssh://` repository URL
- **THEN** the error is shown at Repository URL inside the "Repository" group and the other values are kept

#### Scenario: Groups are labelled regions
- **WHEN** the General tab is rendered
- **THEN** each group is a region whose accessible name is its heading
