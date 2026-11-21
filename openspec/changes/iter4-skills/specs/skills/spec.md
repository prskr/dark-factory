# Spec Delta

## Purpose

Skills managed in Dark Factory: what a skill consists of, its validation, limits and versions, the built-in skills shipped with Dark Factory, the skills page, assignment per persona and project, and which skills a run gets (F13). Delivery into the harness is part of `acp-harness`.

## ADDED Requirements

### Requirement: Skill content
A skill SHALL have a name, a description, a body and zero or more supporting files. The name SHALL be 1–64 characters of `[a-z0-9-]` without a leading, trailing or double hyphen, unique across built-in and instance skills, and SHALL NOT change after creation. The description SHALL be one line of 1–1024 characters. A supporting file SHALL have a relative path without `..` components, with at most 8 components and other than `SKILL.md`, UTF-8 text content and an executable flag. Dark Factory SHALL render the skill's `SKILL.md` as YAML frontmatter with `name` and `description`, followed by the body.

#### Scenario: Valid skill
- **WHEN** a board member saves skill `release-notes` with description "Write release notes from merged pull requests." and a body
- **THEN** the skill is stored and its `SKILL.md` starts with frontmatter `name: release-notes` and that description, followed by the body

#### Scenario: Invalid name
- **WHEN** a board member saves a skill named `Release_Notes`
- **THEN** the form shows a field error for the name and nothing is saved

#### Scenario: Name taken
- **WHEN** a board member creates a skill named like an existing skill
- **THEN** the form shows a field error naming the existing skill and nothing is saved

#### Scenario: Supporting file outside the skill
- **WHEN** a board member adds a supporting file with path `../x.sh`
- **THEN** the form shows a field error for that path and nothing is saved

#### Scenario: Name fixed after creation
- **WHEN** a board member opens an existing skill
- **THEN** the name is shown but cannot be edited

### Requirement: Skill size limits
A skill SHALL be at most 256 KiB (body plus supporting files) with at most 50 supporting files; saving a larger skill SHALL fail with a field error naming the limit. If the skills of a run exceed 4 MiB together, the run SHALL fail before provisioning with a reason naming the limit.

#### Scenario: Skill too large
- **WHEN** a board member saves a skill whose body and files are 300 KiB together
- **THEN** the form shows an error naming the 256 KiB limit and nothing is saved

#### Scenario: Run over the limit
- **WHEN** a run's persona and project together have skills of 5 MiB
- **THEN** the run fails before a sandbox is provisioned and its reason names the 4 MiB limit

### Requirement: Skill versions
Every save of an instance skill SHALL create a new version numbered from 1 and record its author and time. A run SHALL get the current version of each skill at run start and keep it for the whole run. The skill form SHALL list the versions read-only.

#### Scenario: Edit during a run
- **WHEN** run 1 of `DF-3` started with version 2 of `release-notes` and a board member saves version 3 during the run
- **THEN** run 1 keeps version 2 and run 2 gets version 3

#### Scenario: Version list
- **WHEN** a board member opens a skill saved three times
- **THEN** the form lists versions 1 to 3 with author and time

### Requirement: Built-in skills
Dark Factory SHALL ship built-in skills with the release, versioned with the Dark Factory version. Built-in skills SHALL be shown on the skills page with a "Built-in" label and SHALL NOT be editable or deletable. Their names SHALL start with `dark-factory-`, and an instance skill name SHALL NOT start with `dark-factory-`. The built-in `dark-factory-basics` SHALL contain the principles of the agent playbook and SHALL be part of every run.

#### Scenario: Built-in read-only
- **WHEN** a board member opens `dark-factory-basics` on the skills page
- **THEN** its content is shown without save or delete actions

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
The settings area SHALL have a "Skills" page listing built-in and instance skills in two-line rows (name, description), built-ins labeled "Built-in". Board members SHALL create, edit and delete instance skills in a form modal with name, description, body, supporting files (path, content, executable) and the version list. Deleting a skill SHALL ask for confirmation, list the personas and projects that use it, and remove its versions and assignments.

#### Scenario: Create a skill
- **WHEN** a board member creates skill `release-notes` with a supporting file `scripts/collect.sh` marked executable
- **THEN** the list shows `release-notes` with its description and the skill has version 1

#### Scenario: Delete a used skill
- **WHEN** a board member deletes `release-notes`, which persona `linus` and project `DF` use
- **THEN** the confirmation names `linus` and `DF`, and after confirming the skill and both assignments are gone

#### Scenario: Empty list
- **WHEN** no instance skill exists
- **THEN** the page lists the built-in skills and offers to create a skill

### Requirement: Skill changes are audited
Every create, edit and delete of an instance skill SHALL write an audit entry (`skill.create`, `skill.update` with the old and new content, `skill.delete` with the last content). Assignment changes SHALL be part of the audit entry of the persona or project.

#### Scenario: Skill edited
- **WHEN** a board member changes the body of `release-notes`
- **THEN** an audit entry `skill.update` with the old and new body exists

#### Scenario: Skill assigned to a persona
- **WHEN** a board member adds `release-notes` to persona `linus`
- **THEN** the `persona.update` audit entry shows the skill list before and after
