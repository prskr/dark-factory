## MODIFIED Requirements

### Requirement: Runs, PR and issue
The task details SHALL show the task's status line, the pull request and linked issue as links when they exist, project, persona and task branch, and the task's runs with number, state, outcome, detail, start/end time and compact token counts (k and M, the exact value as tooltip) for input, output, cache read and cache write, summed per model from the gateway's usage records, plus the number of model requests with unknown usage when there are any. Runs SHALL be listed newest first. The run number in the runs list SHALL link to the start of that run in the transcript.

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
