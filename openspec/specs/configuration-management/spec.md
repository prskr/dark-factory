# configuration-management Specification

## Purpose
Lets board members configure what agents work on and as whom: projects, personas and references to credentials stored as Kubernetes Secrets. The database is authoritative (ADR-0020).

## Requirements

### Requirement: Projects
Board members SHALL list, create and edit projects with name, key, repository URL (HTTPS), optional issue tracker URL, sandbox image, base branch and forge credential reference. The key SHALL be immutable once the project has tasks.

#### Scenario: Create a project
- **WHEN** a board member submits name "Dark Factory", key `DF`, repo `https://git.example.org/peter/dark-factory.git`, image, base `main` and a forge credential
- **THEN** the project is stored and appears in the project list

#### Scenario: Non-HTTPS repository
- **WHEN** the repository URL uses `ssh://` or `http://`
- **THEN** the form shows a validation error and nothing is stored

#### Scenario: Changing the key of a used project
- **WHEN** a board member edits the key of a project that has tasks
- **THEN** the change is rejected with a validation error

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

### Requirement: Desktop-only configuration pages
Project, persona and credential pages SHALL be reachable on any screen but SHALL NOT appear in the mobile navigation.

#### Scenario: Phone navigation
- **WHEN** the UI is opened at 390 px width
- **THEN** the navigation shows Tasks but not Projects, Personas or Credentials
