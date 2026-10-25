# Spec Delta

## MODIFIED Requirements

### Requirement: Projects
Board members SHALL list, create and edit projects with name, key, repository URL (HTTPS), optional issue tracker URL, sandbox image, optional adapter command, base branch and forge credential reference. The key SHALL be set when the project is created and SHALL be immutable afterwards; the edit form SHALL show it read-only. The adapter command, if set, replaces the instance's harness image and adapter for this project's runs.

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

#### Scenario: Project image with its own adapter
- **WHEN** a board member sets the adapter command `/usr/local/bin/my-acp-adapter` on project `DF`
- **THEN** it is stored and the form shows it on the next edit

#### Scenario: Adapter command left empty
- **WHEN** a board member saves a project with an empty adapter command
- **THEN** the project uses the instance's harness image and adapter

### Requirement: Personas
Board members SHALL list, create and edit personas with name, system prompt, harness (iteration 1: `claude-code`), optional model and harness credential reference. A persona SHALL NOT have an adapter command.

#### Scenario: Create a persona
- **WHEN** a board member creates persona "Linus" with a system prompt and a harness credential
- **THEN** the persona is stored and can be selected when creating a task

#### Scenario: No adapter command on the form
- **WHEN** a board member opens the persona form
- **THEN** it has no adapter command field
