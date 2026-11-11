## MODIFIED Requirements

### Requirement: Projects
Board members SHALL list, create and edit projects with name, key, repository URL (HTTPS), optional issue tracker URL, sandbox image, optional adapter command, base branch and forge credential reference. The key SHALL be set when the project is created and SHALL be immutable afterwards; the project settings page SHALL show it read-only. The adapter command, if set, replaces the instance's harness image and adapter for this project's runs. The adapter command field SHALL use shell-style quoting: arguments are separated by whitespace, single or double quotes keep whitespace inside an argument, and the stored command SHALL be shown quoted so that saving it again stores the same arguments. An unclosed quote SHALL be a validation error.

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

#### Scenario: Argument with a space
- **WHEN** a board member sets the adapter command `/opt/adapter '--config=/workspace/my config.json'` on project `DF` and later saves the settings page without changing it
- **THEN** the command is stored both times as the two arguments `/opt/adapter` and `--config=/workspace/my config.json`

#### Scenario: Unclosed quote
- **WHEN** a board member saves the adapter command `/opt/adapter "--config=a b`
- **THEN** the form shows a validation error at Adapter command and nothing is stored

### Requirement: Configuration forms as modals
Creating a project or a model provider, and creating and editing a persona or credential reference, SHALL happen in a modal over the corresponding list (a full-height sheet below 768 px). A validation error SHALL keep the modal open with the entered values and the error at the field. Saving a persona or credential reference SHALL close the modal and update the list without a full page load. Creating a project or a model provider SHALL close the modal and open the new item's settings page.

#### Scenario: Validation error in the modal
- **WHEN** a credential reference is submitted without a key
- **THEN** the modal stays open with "Key is required." at the Key field and the other values kept

#### Scenario: Saved project
- **WHEN** a board member creates project `WEB` in the "New project" modal
- **THEN** the modal closes and the settings page of `WEB` opens

#### Scenario: Saved model provider
- **WHEN** a board member creates provider "Anthropic work" in the "New model provider" modal
- **THEN** the modal closes and the settings page of "Anthropic work" opens

### Requirement: Project settings page
Each project SHALL have a settings page at `/projects/{key}`, inside the settings area from 768 px and a full page below. It SHALL show a back link to the project list, the project's name, key and forge icon, the tabs General, Security and, for `admin`, Integrations, and one "Save project" button for General and Security. The Security tab holds the project's push checks and hidden paths. The project list's name and edit action SHALL link to it, and an unknown key SHALL answer `404`. The tab orientation follows the approved PenPot board.

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
The open tab of the project settings page SHALL be part of the URL as `tab=security` or `tab=integrations`, and left out for General. Opening such a URL SHALL show that tab, choosing a tab SHALL update the URL without a page load, and an unknown or non-permitted value SHALL show General.

#### Scenario: Link to a tab
- **WHEN** an admin opens `/projects/DF?tab=integrations`
- **THEN** the page opens with the Integrations tab open

#### Scenario: Tab not permitted
- **WHEN** a user with only `board-member` opens `/projects/DF?tab=integrations`
- **THEN** the page opens with the General tab open and shows no Integrations tab

#### Scenario: Choosing a tab updates the URL
- **WHEN** a board member on `/projects/DF` chooses Security
- **THEN** the URL becomes `/projects/DF?tab=security` without a page load

#### Scenario: Old tab value
- **WHEN** a board member opens `/projects/DF?tab=push-checks`
- **THEN** the page opens with the General tab open

### Requirement: Saving the project settings page
Saving SHALL keep the project settings page open with the stored values and confirm the save visibly and to screen readers. A validation error SHALL keep the entered values and open the tab with the first error, and the URL SHALL name that tab.

#### Scenario: Save stays on the page
- **WHEN** a board member changes the base branch of `DF` and saves
- **THEN** the page stays open with the new value, and a "Project saved" status is shown and announced

#### Scenario: Validation error on another tab
- **WHEN** a board member on `/projects/DF?tab=security` saves `DF` with an `ssh://` repository URL entered on General
- **THEN** the page shows the General tab with the error at Repository URL, all entered values kept, and the URL becomes `/projects/DF`

### Requirement: Form tabs on phones
Below 768 px the tab row of a configuration form, the project settings page or the model provider settings page SHALL scroll horizontally when its tabs do not fit, without scrolling the page, and SHALL keep the open tab fully visible when the form opens or a tab is chosen.

#### Scenario: Four tabs at 390 px
- **WHEN** an admin opens the settings page of project `DF` at 390 px width and chooses "Integrations", the last of its tabs
- **THEN** the tab row scrolls so that "Integrations" is fully visible, and the page has no horizontal scrollbar
