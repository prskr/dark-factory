# Spec Delta

## ADDED Requirements

### Requirement: Project overview with task counts
The project list SHALL show per project its key, name, forge, repository, base branch and the number of its tasks per status group: Queued (queued), Running (running, rate limited), Waiting (waiting, limit reached) and Done (completed, failed). Each count SHALL link to the task list filtered by that project and status group.

#### Scenario: Counts of a project
- **WHEN** project `DF` has one queued, two running, one rate-limited, one limit-reached and three failed tasks
- **THEN** its row shows Queued 1, Running 3, Waiting 1 and Done 3

#### Scenario: Count as link
- **WHEN** a board member follows the Waiting count of `DF`
- **THEN** the task list opens with `project=DF` and `status=waiting`

### Requirement: Forge of a project
The UI SHALL show for each project the forge integration that serves it, by icon and name, and a generic Git icon when no forge integration serves it. The forge SHALL be determined by the instance's configured forge integration, not guessed from the repository host.

#### Scenario: Project served by the forgejo integration
- **WHEN** the instance configures the `forgejo` integration and lists project `DF`
- **THEN** the row of `DF` shows the Forgejo icon with the name "Forgejo"

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
Creating and editing a project, persona or credential reference SHALL happen in a modal over the corresponding list (a full-height sheet below 1024 px). A validation error SHALL keep the modal open with the entered values and the error at the field. Saving SHALL close the modal and update the list without a full page load.

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
Project, persona and credential pages SHALL be reachable on any screen. Below 1024 px, the navigation SHALL include Projects but not Personas or Credentials.

#### Scenario: Phone navigation
- **WHEN** the UI is opened at 390 px width
- **THEN** the navigation shows Tasks and Projects but not Personas or Credentials

## REMOVED Requirements

### Requirement: Desktop-only configuration pages
**Reason**: Projects join the phone navigation so a board member can open a project's task list on the phone.
**Migration**: Replaced by "Configuration pages on phones"; Personas and Credentials stay out of the phone navigation.
