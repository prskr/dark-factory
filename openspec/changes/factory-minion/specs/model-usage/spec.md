## MODIFIED Requirements

### Requirement: Usage records
The control plane SHALL store each usage record reported by a gateway with request ID, caller, time, model, wire API, upstream status, input, output, cache-read and cache-write tokens, and whether usage is known. The caller SHALL be either a run, or the minion with its job kind and the project, task and persona the job belongs to (each optional). A minion record whose project, task or persona no longer exists SHALL be stored without that reference. A record with a request ID that is already stored SHALL be ignored.

#### Scenario: Retried report
- **WHEN** a gateway reports the same batch twice after a timeout
- **THEN** each request is stored once

#### Scenario: Run that already ended
- **WHEN** a record arrives for a run that ended a minute ago
- **THEN** it is stored for that run

#### Scenario: Minion record for a persona
- **WHEN** a record of job `persona-description` for persona "Reviewer" arrives
- **THEN** it is stored with caller minion, job `persona-description`, persona "Reviewer" and no run, project or task

### Requirement: Usage per run and model
The usage of a run SHALL be the sum of its known records per model, plus the number of requests with unknown usage. Minion records SHALL NOT count towards any run.

#### Scenario: Two models in one run
- **WHEN** run R has two records for `claude-sonnet-5-5` with 100 and 50 input tokens and one for `claude-haiku-5-5` with 10
- **THEN** R's usage shows 150 input tokens for `claude-sonnet-5-5` and 10 for `claude-haiku-5-5`

#### Scenario: Unknown usage
- **WHEN** run R has one record with unknown usage
- **THEN** R's usage shows one request with unknown usage and no tokens for it

#### Scenario: Minion record of the same task
- **WHEN** task `DF-7` has run R and one minion record of job `task-title`
- **THEN** R's usage does not contain the minion record

## ADDED Requirements

### Requirement: Minion usage per task
The usage section of a task SHALL show one line "Factory minion" with the summed tokens per model of the task's minion records and the number of minion requests with unknown usage, below the run usage. Without minion records the line SHALL NOT be shown.

#### Scenario: Generated title
- **WHEN** task `DF-7` has one minion record of 300 input and 12 output tokens for `claude-haiku-5-5`
- **THEN** the task's usage section shows "Factory minion" with `claude-haiku-5-5`, 300 input and 12 output tokens
