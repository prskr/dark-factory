# Spec Delta

## MODIFIED Requirements

### Requirement: Draft PR on first push ahead of base
On a successful task-branch `PushReport` with at least one commit ahead of base, if the task has no PR, the control plane SHALL open a draft PR from the task branch into the project's base branch, using stored title/body or defaults (`<KEY>: <task title>`, task description plus a `Task: <KEY>` line). The control plane SHALL NOT open a PR for a task in status `completed`, neither on a push report nor in the retry at run end.

#### Scenario: First push
- **WHEN** the runtime reports pushing `factory/task/DF-1` 2 commits ahead of `main`
- **THEN** a draft PR `factory/task/DF-1 → main` exists and its number and URL are stored on the task

#### Scenario: Push without commits ahead
- **WHEN** a push report shows 0 commits ahead
- **THEN** no PR is opened

#### Scenario: Marked done before the first PR
- **WHEN** a board member marks `DF-1` without a PR done and its cancelled run reports a push 2 commits ahead of `main`
- **THEN** no PR is opened and `DF-1` stays `completed`

### Requirement: Ready for review on completion
When a run ends with outcome `completed` and the task has a PR, the control plane SHALL mark the PR ready for review and post the completion summary as a PR comment. The task becomes `in_review`, or `queued` if undelivered messages exist (see "Run end maps to task status"); the PR actions are the same in both cases. Marking a task done by hand SHALL mark the PR ready as well, without a summary.

#### Scenario: Task completed
- **WHEN** run 2 of `DF-1` ends with `completed`
- **THEN** the PR is no longer a draft, carries the summary as a comment, and `DF-1` is `in_review`

#### Scenario: Message waiting at completion
- **WHEN** run 2 of `DF-1` ends with `completed` while a board member's message is still undelivered
- **THEN** the PR is marked ready and gets the summary, and `DF-1` becomes `queued` so the next run picks up the message

#### Scenario: Marked done by hand
- **WHEN** a board member marks `DF-1` with a draft PR done
- **THEN** the PR is no longer a draft and gets no summary comment
