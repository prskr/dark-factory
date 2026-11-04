# Spec Delta

## MODIFIED Requirements

### Requirement: Runs, PR and issue
The task details SHALL show the task's status line, links to the pull request (with an "attached" marker for an attached one) and linked issue when they exist, project, persona, task branch and the runs, newest first. A run shows number, state, outcome, detail, start/end time, compact token counts (k and M, exact value as tooltip) for input, output, cache read and cache write per model from the gateway's usage records, and the number of requests with unknown usage. The run number SHALL link to that run in the transcript.

#### Scenario: Rate-limited run
- **WHEN** a run ended with outcome `rate_limited` and a retry time
- **THEN** the run list shows "rate limited" and the retry time

#### Scenario: PR opened
- **WHEN** the draft PR is opened while the page is open
- **THEN** the PR link appears without a reload

#### Scenario: Run order
- **WHEN** a task has runs #1, #2 and #3
- **THEN** the list shows #3 first and #1 last

#### Scenario: Jump to a run
- **WHEN** a board member clicks the number of a run in the runs list
- **THEN** the transcript scrolls to the divider of that run and does not jump back to the latest item

#### Scenario: Token table
- **WHEN** a run used model `claude-sonnet-5-5` with 182,000 input tokens
- **THEN** the run card shows the model name on its own line, `182k` under "in", and the tooltip shows `182000`

#### Scenario: Requests without usage
- **WHEN** two model requests of a run reported no usage
- **THEN** the run card shows "2 requests without usage" below the token table

#### Scenario: Usage refreshed at turn end
- **WHEN** a turn of a running run ends while the page is open
- **THEN** the run card shows the usage stored up to then without a reload

#### Scenario: Attached PR
- **WHEN** the details of `DF-13`, attached to Renovate's PR #40, are shown
- **THEN** the PR link is followed by an "attached" marker and the task branch is `renovate/serde-1.x`


### Requirement: New task composer
The UI SHALL offer a new-task composer, opened from the new-task action of the rail or bottom bar, with project, assignee (persona), one description field and an options menu holding the optional linked issue and the optional existing branch. Submitting it SHALL create the task through the task-management operations and open the new task.

#### Scenario: Create DF-1
- **WHEN** a board member submits the composer for project `DF` with a description
- **THEN** the new task `DF-1` opens with status `queued`

#### Scenario: Missing description
- **WHEN** the composer is submitted with an empty description
- **THEN** the composer stays open with an error and the chosen project and assignee kept

#### Scenario: Linked issue set
- **WHEN** issue 7 is entered in the options menu
- **THEN** the composer shows "Issue #7" as a removable chip and the task is created with issue number 7

#### Scenario: Existing branch set
- **WHEN** branch `renovate/serde-1.x` is entered in the options menu
- **THEN** the composer shows "Branch renovate/serde-1.x" as a removable chip and the task is created on that branch

#### Scenario: Rejected branch
- **WHEN** the composer is submitted with branch `renovate/serde-1.x` while `DF-13` holds it
- **THEN** the composer stays open with an error naming `DF-13` and the entered values kept
