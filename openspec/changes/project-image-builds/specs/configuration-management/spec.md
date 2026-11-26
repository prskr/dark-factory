## MODIFIED Requirements

### Requirement: Projects
Board members SHALL list, create and edit projects with name, key, repository URL (HTTPS), optional issue tracker URL, project image source (see "Project image source"), optional adapter command, base branch, forge credential reference and the branch settings (see "Project branch settings"). The key SHALL be set at creation and be immutable; the edit form SHALL show it read-only. An adapter command replaces the instance's harness image and adapter for the project's runs.

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

## ADDED Requirements

### Requirement: Project image source
The project form SHALL offer the project image as one choice: "Image" with a required image reference, "Build from repository" (only while image builds are enabled, see `image-builds`), or "Instance default" (only while a default image is configured). Projects that existed before this setting SHALL keep their image as "Image". Switching away from "Build from repository" SHALL cancel the project's running build and keep its build history.

#### Scenario: New project on the default image
- **WHEN** a default image is configured and a board member creates a project with "Instance default"
- **THEN** the project is stored without an image reference

#### Scenario: Image choice without reference
- **WHEN** a board member saves "Image" with an empty reference
- **THEN** the form shows a validation error and nothing is stored

#### Scenario: No default configured
- **WHEN** no default image is configured
- **THEN** the form doesn't offer "Instance default"

### Requirement: Default image
The control plane SHALL read the optional instance setting `sandbox.defaultImage`. Projects with the image source "Instance default" SHALL run it.

#### Scenario: Default image used
- **WHEN** `sandbox.defaultImage` is `ghcr.io/example/base:1` and project `DF` uses "Instance default"
- **THEN** runs of `DF` use `ghcr.io/example/base:1`

### Requirement: Project without usable image source
A project SHALL count as misconfigured when it uses "Instance default" and no default image is configured, or "Build from repository" and image builds are not enabled. At startup the control plane SHALL log one warning listing these projects; it SHALL still start. The project's settings page SHALL show the problem.

#### Scenario: Default image removed
- **WHEN** the operator removes `sandbox.defaultImage` while project `DF` uses "Instance default" and the control plane restarts
- **THEN** the startup log warns naming `DF` and the settings page of `DF` shows that it has no project image
