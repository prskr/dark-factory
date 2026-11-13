# Spec Delta

## MODIFIED Requirements

### Requirement: Projects
Board members SHALL list, create and edit projects with name, key, repository URL (HTTPS), optional issue tracker URL, sandbox image, optional adapter command, base branch, forge credential reference, branch rewrite policy (`never` or `with-lease`, default `with-lease`) and agent branch globs (default `renovate/**` and `dependabot/**`, see `task-branches`). The key SHALL be set when the project is created and SHALL be immutable afterwards; the edit form SHALL show it read-only. The adapter command, if set, replaces the instance's harness image and adapter for this project's runs.

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

#### Scenario: Default rewrite policy
- **WHEN** a board member creates a project without changing the branch rewrite policy
- **THEN** the project's policy is `with-lease`

#### Scenario: Rewrite policy changed
- **WHEN** a board member sets the branch rewrite policy of `DF` to `never`
- **THEN** it is stored, audited with before and after values, and applies from the next run of each task

#### Scenario: Default agent branches
- **WHEN** a board member creates a project without changing the agent branches
- **THEN** the project's agent branch globs are `renovate/**` and `dependabot/**`

#### Scenario: Invalid agent branch glob
- **WHEN** a board member saves the agent branches with the line `renovate/[`
- **THEN** the form shows a validation error naming the line and nothing is stored

#### Scenario: Agent branches changed
- **WHEN** a board member adds `agent/**` to the agent branches of `DF`
- **THEN** it is stored and audited with before and after values
