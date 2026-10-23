# configuration-management Specification

## Purpose
Lets board members configure what agents work on and as whom: projects, personas and references to credentials stored as Kubernetes Secrets. The database is authoritative (ADR-0020).

## Requirements

### Requirement: Projects
Board members SHALL list, create and edit projects with name, key, repository URL (HTTPS), optional issue tracker URL, sandbox image, base branch and forge credential reference. The key SHALL be set when the project is created and SHALL be immutable afterwards; the edit form SHALL show it read-only.

#### Scenario: Create a project
- **WHEN** a board member submits name "Dark Factory", key `DF`, repo `https://git.example.org/peter/dark-factory.git`, image, base `main` and a forge credential
- **THEN** the project is stored and appears in the project list

#### Scenario: Non-HTTPS repository
- **WHEN** the repository URL uses `ssh://` or `http://`
- **THEN** the form shows a validation error and nothing is stored

#### Scenario: Key in the edit form
- **WHEN** a board member opens the edit form of project `DF`, with or without tasks
- **THEN** the key is shown read-only

#### Scenario: Changing the key of a used project
- **WHEN** a board member edits the key of a project that has tasks
- **THEN** the change is rejected with a validation error

#### Scenario: Changing the key of an unused project
- **WHEN** an edit request for project `DF`, which has no tasks, carries a different key
- **THEN** the change is rejected with a validation error and nothing is stored

### Requirement: Personas
Board members SHALL list, create and edit personas with name, system prompt, harness (iteration 1: `claude-code`), adapter command, optional model and harness credential reference.

#### Scenario: Create a persona
- **WHEN** a board member creates persona "Linus" with a system prompt and a harness credential
- **THEN** the persona is stored and can be selected when creating a task

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
The UI SHALL show for each project the forge integration that serves it, by icon and name, and a generic Git icon when no forge integration serves it. The forge SHALL be determined by the instance's configured forge integration, not guessed from the repository host.

#### Scenario: Project served by the forgejo integration
- **WHEN** the instance configures the `forgejo` integration and project `DF` has a forge credential
- **THEN** the row of `DF` shows the Forgejo icon with the name "Forgejo"

#### Scenario: Project without forge credential
- **WHEN** the instance configures the `forgejo` integration and project `WEB` has no forge credential
- **THEN** the row of `WEB` shows the generic Git icon

#### Scenario: No forge integration
- **WHEN** the instance configures no forge integration
- **THEN** every project shows the generic Git icon

### Requirement: Forge feedback in the project form
While a board member types the repository URL or the issue tracker URL in the project form, the form SHALL show which forge integration the URL maps to, by icon and a one-line hint, and a generic icon with a hint that PR and issue features are unavailable when it maps to none.

#### Scenario: Typing a repository URL
- **WHEN** a board member enters `https://git.example.com/factory/dark-factory.git` on an instance with the `forgejo` integration
- **THEN** the field shows the Forgejo icon and the hint names the forgejo integration

#### Scenario: Not an HTTPS repository
- **WHEN** the URL is not a valid HTTPS repository URL
- **THEN** the field shows the generic Git icon and the hint says no forge integration applies

### Requirement: Persona cards
The persona list SHALL show each persona as a card with its name, harness, model (or "default"), the start of its system prompt and its harness credential reference.

#### Scenario: Persona card
- **WHEN** persona "Linus" with model `opus` and credential `anthropic-api` exists
- **THEN** its card shows "Linus", "Claude Code · opus", the first lines of its prompt and `anthropic-api`

### Requirement: Configuration forms as modals
Creating and editing a project, persona or credential reference SHALL happen in a modal over the corresponding list (a full-height sheet below 768 px). A validation error SHALL keep the modal open with the entered values and the error at the field. Saving SHALL close the modal and update the list without a full page load.

#### Scenario: Validation error in the modal
- **WHEN** a credential reference is submitted without a key
- **THEN** the modal stays open with "Key is required." at the Key field and the other values kept

#### Scenario: Saved project
- **WHEN** a board member saves a new project in the modal
- **THEN** the modal closes and the project appears in the list

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
