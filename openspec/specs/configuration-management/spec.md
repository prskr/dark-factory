# configuration-management Specification

## Purpose
Lets board members configure what agents work on and as whom: projects, personas and references to credentials stored as Kubernetes Secrets. The database is authoritative (ADR-0020).

## Requirements

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

### Requirement: Personas
Board members SHALL list, create and edit personas with name, system prompt, harness (`claude-code` or `opencode`), a required model provider and a required model of that provider, both compatible with the harness. A persona SHALL NOT have an adapter command.

#### Scenario: Create a persona
- **WHEN** a board member creates persona "Linus" with a system prompt, model provider "Claude subscription" and model `claude-sonnet-5-5`
- **THEN** the persona is stored and can be selected when creating a task

#### Scenario: Create an OpenCode persona
- **WHEN** a board member creates persona "Ada" with harness `opencode`, model provider "Go" and model `glm-5.3`
- **THEN** the persona is stored and its runs start OpenCode

#### Scenario: Model is required
- **WHEN** a board member saves a persona without a model
- **THEN** the form rejects it

#### Scenario: Model select follows the provider
- **WHEN** a board member selects model provider "Go" in the persona form
- **THEN** the model select lists only Go's models whose wire API the persona's harness can use

#### Scenario: No adapter command on the form
- **WHEN** a board member opens the persona form
- **THEN** it has no adapter command field

#### Scenario: Existing persona after the upgrade
- **WHEN** persona "Linus" referenced harness credential `claude` before the upgrade
- **THEN** after migrating it references a model provider with scheme `claude-subscription`, Anthropic Messages at `https://api.anthropic.com` and credential reference `claude`

#### Scenario: Persona model becomes a provider model
- **WHEN** persona "Linus" with model `claude-sonnet-5-5` and model provider "Claude subscription" is migrated
- **THEN** "Claude subscription" has model `claude-sonnet-5-5` on Anthropic Messages without limits and "Linus" references it

#### Scenario: Persona without a model blocks the migration
- **WHEN** a persona has no model when the migration runs
- **THEN** the migration fails and names the persona

### Requirement: Credential references never hold values
Board members SHALL create and edit credential references consisting of a name, a Kubernetes Secret name and a key. The UI and database SHALL never accept, store or display secret values.

#### Scenario: Create a reference
- **WHEN** a board member creates reference "claude-subscription" → Secret `claude`, key `token`
- **THEN** only name, Secret name and key are stored

#### Scenario: No value field
- **WHEN** the credential form is rendered
- **THEN** it contains no input for a secret value

### Requirement: Configuration changes are audited
Every create or edit of a project, persona or credential reference SHALL write an audit entry with the before/after values (secret values never exist to include).

#### Scenario: Persona edited
- **WHEN** a board member changes a persona's system prompt
- **THEN** an audit entry `persona.update` with the old and new prompt exists

### Requirement: Project overview with task counts
The project list SHALL show per project its key, name, forge, repository, base branch and the number of its tasks per status group: Queued (queued), Running (running, rate limited), Waiting (waiting, limit reached) and Done (completed, failed). Each count SHALL link to the task list filtered by that project and exactly the statuses the count includes.

#### Scenario: Counts of a project
- **WHEN** project `DF` has one queued, two running, one rate-limited, one limit-reached and three failed tasks
- **THEN** its row shows Queued 1, Running 3, Waiting 1 and Done 3

#### Scenario: Count as link
- **WHEN** a board member follows the Waiting count of `DF`
- **THEN** the task list opens with `project=DF` and `status=waiting`

#### Scenario: Running count as link
- **WHEN** a board member follows the Running count of `DF`
- **THEN** the task list opens with `project=DF` and `status=running` and lists only running and rate-limited tasks of `DF`

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

### Requirement: Persona cards
The persona list SHALL show each persona as a card with its name, harness, model (or "default"), the start of its system prompt and its harness credential reference.

#### Scenario: Persona card
- **WHEN** persona "Linus" with model `opus` and credential `anthropic-api` exists
- **THEN** its card shows "Linus", "Claude Code · opus", the first lines of its prompt and `anthropic-api`

### Requirement: Configuration forms as modals
Creating a project, and creating and editing a persona or credential reference, SHALL happen in a modal over the corresponding list (a full-height sheet below 768 px). A validation error SHALL keep the modal open with the entered values and the error at the field. Saving a persona or credential reference SHALL close the modal and update the list without a full page load. Creating a project SHALL close the modal and open the new project's settings page.

#### Scenario: Validation error in the modal
- **WHEN** a credential reference is submitted without a key
- **THEN** the modal stays open with "Key is required." at the Key field and the other values kept

#### Scenario: Saved project
- **WHEN** a board member creates project `WEB` in the "New project" modal
- **THEN** the modal closes and the settings page of `WEB` opens

### Requirement: Empty configuration lists
When there are no projects, personas or credential references, the corresponding list SHALL explain in one sentence what the item is for and offer its create action. The empty project list SHALL also point to credential references for private repositories.

#### Scenario: No projects yet
- **WHEN** the project list is opened on a new instance
- **THEN** it shows "No projects yet", an explanation, a New project button and a link to credential references

### Requirement: Configuration pages on phones
Project, persona and credential pages SHALL be reachable on any screen. Below 768 px, the navigation SHALL include Projects but not Personas or Credentials.

#### Scenario: Phone navigation
- **WHEN** the UI is opened at 390 px width
- **THEN** the navigation shows Tasks and Projects but not Personas or Credentials

### Requirement: Form tabs on phones
Below 768 px the tab row of a configuration form or the project settings page SHALL scroll horizontally when its tabs do not fit, without scrolling the page, and SHALL keep the open tab fully visible when the form opens or a tab is chosen.

#### Scenario: Four tabs at 390 px
- **WHEN** an admin opens the settings page of project `DF` at 390 px width and chooses "Integrations", the last of its tabs
- **THEN** the tab row scrolls so that "Integrations" is fully visible, and the page has no horizontal scrollbar

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

### Requirement: Webhook wake policy of a project
The project form SHALL offer the setting "Wake tasks on reviews and mentions" with `none`, `members` and `anyone`, defaulting to `members` for new and existing projects. Changes SHALL be audited like other project changes.

#### Scenario: Existing project after the migration
- **WHEN** project `DF` existed before this change
- **THEN** its setting is `members`

#### Scenario: Open project to outside reviews
- **WHEN** a board member sets the setting of `DF` to `anyone`
- **THEN** it is stored, audited and shown on the next edit

### Requirement: Persona description
A persona SHALL have an optional plain-text description of its role and strengths, at most 500 characters, edited in the persona form and empty by default. The persona card SHALL show the description when it is set, in place of the start of the system prompt. Agents read it through `list_personas`.

#### Scenario: Description on the card
- **WHEN** a board member saves persona "Reviewer" with description "Reviews Rust PRs for correctness and tests"
- **THEN** the card of "Reviewer" shows that description instead of the start of its system prompt

#### Scenario: No description
- **WHEN** persona "Linus" has no description
- **THEN** its card shows the start of its system prompt as before

#### Scenario: Too long
- **WHEN** a description of 501 characters is saved
- **THEN** the form shows a validation error and the persona is unchanged

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
The General tab of the project settings page and the new-project form SHALL show their fields in the groups "Project", "Repository", "Sandbox" and "Forge events", in this order, as on the approved PenPot boards ("Projects · settings page, general", "Projects · new project"). Each group SHALL have a heading and SHALL be a region named by it, with a horizontal separator between consecutive groups. Grouping SHALL NOT change field names, validation or what saving stores.

#### Scenario: Groups on the settings page
- **WHEN** a board member opens the settings page of project `DF` on the General tab
- **THEN** "Project" holds Key and Name, "Repository" holds Repository URL, Base branch, Forge credential and Issue tracker URL (in this order), "Sandbox" holds Sandbox image and Adapter command, and "Forge events" holds Wake tasks on reviews and mentions, with a separator between consecutive groups and none above the first

#### Scenario: Groups in the new-project form
- **WHEN** a board member opens "New project"
- **THEN** the form shows the same groups, and Key is editable

#### Scenario: Validation error inside a group
- **WHEN** a project is submitted with an `ssh://` repository URL
- **THEN** the error is shown at Repository URL inside the "Repository" group and the other values are kept

#### Scenario: Groups are labelled regions
- **WHEN** the General tab is rendered
- **THEN** each group is a region whose accessible name is its heading
