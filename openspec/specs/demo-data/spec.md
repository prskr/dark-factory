# demo-data Specification

## Purpose
Defines the demo profile of the seed: realistic projects, personas, tasks, transcripts and artifacts for screenshots and demos of a local instance, written idempotently and separate from the end-to-end fixtures.

## Requirements

### Requirement: Demo projects and personas
The seed SHALL have a demo profile (`seed --demo`, `just seed-demo`) that writes into the configured database at least 3 projects with plausible names, repository URLs and base branches, and at least 3 personas with plausible names, descriptions and model providers.

#### Scenario: Projects and personas
- **WHEN** a developer runs `just seed-demo` against an empty, migrated database
- **THEN** the project list shows 3 demo projects and the persona list 3 demo personas

### Requirement: Demo tasks
The demo profile SHALL write tasks across the demo projects in each of the states queued, running or waiting, blocked with a pending question, rate limited, awaiting dependencies, in review with a PR, completed with a merged PR, failed and cancelled. It SHALL include one parent task with subtasks, one task with a dependency, one task attached to a PR opened outside the factory, and one task with a linked issue. Titles and descriptions SHALL read like real software work.

#### Scenario: Task list
- **WHEN** the task list with "All" is opened after `just seed-demo`
- **THEN** it shows tasks in every listed state across the 3 demo projects

#### Scenario: States survive the local control plane
- **WHEN** `just serve-local` (no cluster) runs for 2 minutes after `just seed-demo`
- **THEN** no demo task changed its state and no run was started for a demo task

### Requirement: Demo task contents
For every demo task that has run, the demo profile SHALL write runs with outcomes, transcript events (user messages, agent messages, tool calls) and token usage per model. At least two tasks SHALL have artifacts, together a Markdown report, an image and a log. Transcript text SHALL read like real work, and timestamps SHALL lie in the recent past relative to the time of seeding.

#### Scenario: Seeding a fresh database
- **WHEN** a developer runs `just seed-demo` against an empty, migrated `serve-local` database
- **THEN** the task list shows tasks in all listed states across 3 projects, and the details of the in-review task show runs, token usage, a PR link and its artifacts

#### Scenario: Artifacts
- **WHEN** the artifacts tab of the task with the report is opened
- **THEN** it lists a Markdown report, an image and a log, and each opens with its content

### Requirement: Demo profile is idempotent
Running the demo profile again SHALL NOT create duplicate projects, personas, tasks, runs or artifacts and SHALL NOT write audit entries for records that didn't change.

#### Scenario: Second run
- **WHEN** `just seed-demo` runs twice against the same database
- **THEN** the number of projects, personas, tasks, runs and artifacts is the same after both runs

### Requirement: Separate from the end-to-end fixtures
The demo profile SHALL use project keys and persona names that the end-to-end fixtures don't use, and SHALL NOT change records of the end-to-end fixtures. The seed without `--demo` SHALL keep writing only the end-to-end fixtures.

#### Scenario: Both profiles in one database
- **WHEN** `just seed` and `just seed-demo` both run against the `serve-local` database
- **THEN** the end-to-end fixtures are unchanged and the demo projects exist next to them
