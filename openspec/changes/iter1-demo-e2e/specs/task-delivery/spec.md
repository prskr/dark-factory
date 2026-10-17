# Spec Delta

## Purpose

Describes, from a board member's point of view, how an assigned task becomes a pull request in the forge and how follow-up messages continue the same work — the definition of done for iteration 1.

## ADDED Requirements

### Requirement: Assignment leads to a draft PR
When a board member creates a task for a project linked to a Forgejo repository and assigns it to a persona, the system SHALL cold-start a sandbox, work on branch `factory/task/<KEY>`, stream the transcript live, and open a draft PR into the base branch after the first pushed commit.

#### Scenario: DF-1 with the fake agent
- **WHEN** task `DF-1` is created with persona "e2e" whose fake-agent script commits a file and ends its turn
- **THEN** within 5 minutes the transcript shows the agent's messages, `factory/task/DF-1` exists on Forgejo and a draft PR `factory/task/DF-1 → main` is linked on the task

### Requirement: Agent shapes PR and issue
The agent SHALL be able to rephrase the PR title/body and comment on the linked issue through factory tools during the run.

#### Scenario: Tools during the first run
- **WHEN** the script calls `update_pull_request` and `comment_on_issue`
- **THEN** the PR has the new title and the linked issue has the comment

### Requirement: Follow-up resumes the task
A follow-up message on a waiting task SHALL start a new run on the same task volume with the task's history, and its new commits SHALL update the same PR.

#### Scenario: Follow-up message
- **WHEN** the board member sends "also add a test for X" after run 1 ended with `end_turn`
- **THEN** run 2 starts, the transcript shows the message delivered, run 2 resumes the harness session, its commit appears on the same PR and no second PR exists

### Requirement: Completion marks the PR ready
When the agent completes the task, the PR SHALL be marked ready for review and the task status SHALL be `completed`.

#### Scenario: complete_task in run 2
- **WHEN** run 2's script calls `complete_task`
- **THEN** the task is `completed` and the PR is no longer a draft

### Requirement: Rate limit is resumable
A run hitting the provider rate limit SHALL end cleanly with a visible, distinct end reason and the task SHALL continue with the next message.

#### Scenario: Rate-limited run
- **WHEN** the script simulates a rate limit
- **THEN** the run shows "rate limited" with the retry time, the task is `rate_limited`, and a new message starts a run that continues the work
