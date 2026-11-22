# Spec Delta

## Purpose

Skills managed in Dark Factory: what a skill consists of, its validation, limits and versions, the built-in skills shipped with Dark Factory, the skills page, assignment per persona and project, and which skills a run gets (F13). Delivery into the harness is part of `acp-harness`.

## ADDED Requirements

### Requirement: Skill name and description
A skill SHALL have a name, a description, a body and zero or more supporting files. The name SHALL be 1–64 characters of `[a-z0-9-]` without a leading, trailing or double hyphen, unique across built-in and instance skills, and SHALL NOT change after creation. The description SHALL be one line of 1–1024 characters.

#### Scenario: Invalid name
- **WHEN** a board member saves a skill named `Release_Notes`
- **THEN** the form shows a field error for the name and nothing is saved

#### Scenario: Name taken
- **WHEN** a board member creates a skill named like an existing skill
- **THEN** the form shows a field error naming the existing skill and nothing is saved

#### Scenario: Name fixed after creation
- **WHEN** a board member opens an existing skill
- **THEN** the name is shown but cannot be edited

### Requirement: Supporting files
A supporting file SHALL have a relative path of at most 255 bytes, without NUL bytes, and 1–8 normal components (no empty, `.` or `..` component), UTF-8 text content and an executable flag. Paths SHALL be unique within the skill, no path SHALL be an ancestor of another, and no path SHALL be `SKILL.md` or start with `SKILL.md/`.

#### Scenario: Valid supporting files
- **WHEN** a board member adds `scripts/collect.sh` and `references/style.md` to a skill
- **THEN** the skill is saved with both files

#### Scenario: Supporting file outside the skill
- **WHEN** a board member adds a supporting file with path `../x.sh`
- **THEN** the form shows a field error for that path and nothing is saved

#### Scenario: NUL byte in a path
- **WHEN** a board member submits a supporting file whose path contains a NUL byte
- **THEN** the form shows a field error for that path and nothing is saved

#### Scenario: Path below SKILL.md
- **WHEN** a board member adds a supporting file with path `SKILL.md/notes.txt`
- **THEN** the form shows a field error for that path and nothing is saved

#### Scenario: File and directory with the same path
- **WHEN** a board member adds the files `scripts` and `scripts/collect.sh`
- **THEN** the form shows a field error naming both paths and nothing is saved

### Requirement: Rendered SKILL.md
Dark Factory SHALL render a skill's `SKILL.md` as YAML frontmatter with `name` and `description`, followed by the body. Users SHALL NOT write the frontmatter themselves.

#### Scenario: Valid skill
- **WHEN** a board member saves skill `release-notes` with description "Write release notes from merged pull requests." and a body
- **THEN** the skill is stored and its `SKILL.md` starts with frontmatter `name: release-notes` and that description, followed by the body

#### Scenario: Description with YAML characters
- **WHEN** the description is `Use when: the PR is "ready"`
- **THEN** the rendered frontmatter is valid YAML and its `description` is exactly that text

### Requirement: Skill size limits
Sizes SHALL count everything a skill adds to `Assign` (name, version, rendered `SKILL.md`, paths and contents of supporting files). A skill SHALL be at most 256 KiB with at most 50 supporting files; saving a larger skill SHALL fail with a field error naming the limit. If the skills of a run exceed 4 MiB together when its `Assign` is first built, the run SHALL fail before the harness starts with a reason naming the limit.

#### Scenario: Skill too large
- **WHEN** a board member saves a skill whose body and files are 300 KiB together
- **THEN** the form shows an error naming the 256 KiB limit and nothing is saved

#### Scenario: Run over the limit
- **WHEN** a run's persona and project together have skills of 5 MiB
- **THEN** the run ends `FAILED` before the harness starts, its reason names the 4 MiB limit, and the runtime does not reconnect

#### Scenario: Edit during a run does not fail a reconnect
- **WHEN** a run started with 3 MiB of skills, a board member grows one of them so the current versions are 5 MiB, and the agent reconnects
- **THEN** the reconnect gets the versions the run started with and the run continues

### Requirement: Skill versions
Every save of an instance skill SHALL create a new version numbered from 1 and record its author and time. A run SHALL get the current version of each skill and the built-in skills when its `Assign` is first built, and SHALL keep this content for the whole run, also on reconnects and after the skill is deleted. The General tab of the skill settings page SHALL list the versions read-only.

#### Scenario: Edit during a run
- **WHEN** run 1 of `DF-3` started with version 2 of `release-notes` and a board member saves version 3 during the run
- **THEN** run 1 keeps version 2 and run 2 gets version 3

#### Scenario: Edit between provisioning and the first Assign
- **WHEN** run 1 of `DF-3` is provisioned while `release-notes` is at version 2, and a board member saves version 3 before the agent first connects
- **THEN** the first `Assign` of run 1 carries version 3

#### Scenario: Skill deleted during a run
- **WHEN** a board member deletes `release-notes` while run 1 of `DF-3`, which got it, is running, and the agent reconnects
- **THEN** the reconnect's `Assign` still carries `release-notes` with the content run 1 started with

#### Scenario: Reconnect after a release upgrade
- **WHEN** run 1 started on a control plane whose `dark-factory-basics` differs from the release that serves the agent's reconnect
- **THEN** the reconnect's `Assign` carries the `dark-factory-basics` content and version run 1 started with

#### Scenario: Version list
- **WHEN** a board member opens a skill saved three times
- **THEN** its General tab lists versions 1 to 3 with author and time

### Requirement: Built-in skills
Dark Factory SHALL ship built-in skills with the release, versioned with the Dark Factory version. Built-in skills SHALL be shown on the skills page with a "Built-in" label and SHALL NOT be editable or deletable. Their names SHALL start with `dark-factory-`, and an instance skill name SHALL NOT start with `dark-factory-`. The built-in `dark-factory-basics` SHALL contain the principles of the agent playbook and SHALL be part of every run.

#### Scenario: Built-in read-only
- **WHEN** a board member opens `dark-factory-basics` on the skills page
- **THEN** its settings page shows the description, the rendered body and the files, without save or delete actions

#### Scenario: Reserved prefix
- **WHEN** a board member creates an instance skill named `dark-factory-review`
- **THEN** the form shows a field error for the name and nothing is saved

#### Scenario: Basics in every run
- **WHEN** a run starts for a persona and a project without assigned skills
- **THEN** the run's skills are exactly `dark-factory-basics` at the Dark Factory version

### Requirement: Skill assignment
Board members SHALL assign instance skills to personas and to projects. The skills of a run SHALL be the built-in skills that apply to every run, the skills of the run's persona and the skills of the run's project, without duplicates, ordered by name. Chat runs SHALL get the same skills as task runs of the same persona and project.

#### Scenario: Persona and project skills
- **WHEN** persona `linus` has skills `rust-style` and `release-notes` and project `DF` has `release-notes` and `deploy-checks`, and a run of `DF-3` by `linus` starts
- **THEN** the run's skills are `dark-factory-basics`, `deploy-checks`, `release-notes` and `rust-style`

#### Scenario: Assignment changed during a run
- **WHEN** a board member removes `rust-style` from `linus` while run 1 of `DF-3` is running
- **THEN** run 1 keeps the skill and run 2 does not get it

#### Scenario: Chat run
- **WHEN** a chat of project `DF` with persona `linus` starts a run
- **THEN** the run gets the same skills as a task run of `linus` in `DF`

### Requirement: Skills page
The settings area SHALL have a "Skills" page listing built-in and instance skills in two-line rows (name, description), built-ins labeled "Built-in"; each row SHALL link to the skill settings page. A "New skill" modal SHALL ask for name and description only and, on save, open the new skill's settings page on the Body tab. Deleting a skill SHALL ask for confirmation, list the personas and projects that use it, and remove its versions and assignments.

#### Scenario: Create a skill
- **WHEN** a board member creates skill `release-notes` with a description in the "New skill" modal
- **THEN** `/skills/release-notes?tab=body` opens, the skill has version 1, and the list shows `release-notes` with its description

#### Scenario: Delete a used skill
- **WHEN** a board member deletes `release-notes`, which persona `linus` and project `DF` use
- **THEN** the confirmation names `linus` and `DF`, and after confirming the skill and both assignments are gone

#### Scenario: Empty list
- **WHEN** no instance skill exists
- **THEN** the page lists the built-in skills and offers to create a skill

### Requirement: Skill settings page
Each skill SHALL have a settings page at `/skills/{name}`, built like the persona settings page of `markdown-editor`: back link, name, tabs General (name read-only, description, size, versions), Body and Files (path, plain text content and executable flag of each supporting file), and one "Save skill" button. The open tab SHALL be in the URL (`tab=body`, `tab=files`; none for General). Saving SHALL behave as "Saving the persona settings page".

#### Scenario: Add a supporting file
- **WHEN** a board member adds `scripts/collect.sh` marked executable on the Files tab of `release-notes` and saves
- **THEN** the page stays on the Files tab with the file, and the skill has a new version

#### Scenario: Validation error on another tab
- **WHEN** a board member on `/skills/release-notes?tab=body` saves after clearing the description on General
- **THEN** the page shows the General tab with the error at Description, the edited body is kept, and the URL becomes `/skills/release-notes`

#### Scenario: Unknown skill
- **WHEN** `/skills/<unknown name>` is opened
- **THEN** the response is `404`

### Requirement: Skill body in the Markdown editor
The Body tab SHALL hold only the skill body, edited in a Markdown field with the side-by-side layout ("Side-by-side layout" in `markdown-editor`). The stored body SHALL be the text exactly as typed. Supporting files SHALL NOT use the Markdown field.

#### Scenario: Write a skill body
- **WHEN** a board member writes a body with a heading and a `bash` fence on the Body tab and saves
- **THEN** the body is stored unchanged and the rendered `SKILL.md` contains it after the frontmatter

### Requirement: Skill changes are audited
Every create, edit and delete of an instance skill SHALL write an audit entry (`skill.create`, `skill.update` with the old and new content, `skill.delete` with the last content). Assignment changes SHALL be part of the audit entry of the persona or project.

#### Scenario: Skill edited
- **WHEN** a board member changes the body of `release-notes`
- **THEN** an audit entry `skill.update` with the old and new body exists

#### Scenario: Skill assigned to a persona
- **WHEN** a board member adds `release-notes` to persona `linus`
- **THEN** the `persona.update` audit entry shows the skill list before and after
