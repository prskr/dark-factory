## MODIFIED Requirements

### Requirement: Ready for review on completion
When a run ends with outcome `completed` and the task has a PR, the control plane SHALL mark the PR ready for review and post the completion summary as a PR comment. The task becomes `in_review`, or `queued` if undelivered messages exist (see "Run end maps to task status"); the PR actions are the same in both cases. Marking a task done by hand SHALL NOT change the PR.

#### Scenario: Task completed
- **WHEN** run 2 of `DF-1` ends with `completed`
- **THEN** the PR is no longer a draft, carries the summary as a comment, and `DF-1` is `in_review`

#### Scenario: Message waiting at completion
- **WHEN** run 2 of `DF-1` ends with `completed` while a board member's message is still undelivered
- **THEN** the PR is marked ready and gets the summary, and `DF-1` becomes `queued` so the next run picks up the message
