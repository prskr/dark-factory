# Spec Delta

## MODIFIED Requirements

### Requirement: Draft PR on first push ahead of base
On a successful task-branch `PushReport` with at least one commit ahead of base, if the task has no PR, the control plane SHALL open a draft PR from the task's stored branch into the task's base branch, using stored title/body or defaults (`<KEY>: <task title>`, task description plus a `Task: <KEY>` line). The control plane SHALL NOT open a PR for a task in status `completed` or `cancelled`, neither on a push report nor in the retry at run end.

#### Scenario: First push
- **WHEN** the runtime reports pushing `factory/task/DF-1` 2 commits ahead of `main`
- **THEN** a draft PR `factory/task/DF-1 → main` exists and its number and URL are stored on the task

#### Scenario: First push on an existing branch without a PR
- **WHEN** the runtime reports pushing `fix/login` of `DF-14`, which has no PR, 1 commit ahead of `main`
- **THEN** a draft PR `fix/login → main` exists and is not attached

#### Scenario: Push without commits ahead
- **WHEN** a push report shows 0 commits ahead
- **THEN** no PR is opened

#### Scenario: Cancelled before the first PR
- **WHEN** a board member cancels `DF-1` without a PR and its stopped run reports a push 2 commits ahead of `main`
- **THEN** no PR is opened and `DF-1` stays `cancelled`

#### Scenario: Marked done before the first PR
- **WHEN** a board member marks `DF-1` without a PR done and its cancelled run reports a push 2 commits ahead of `main`
- **THEN** no PR is opened and `DF-1` stays `completed`

### Requirement: Idempotent creation
If an open PR for the same head and base already exists (creation conflict, or a crash between creating and recording), the control plane SHALL record that PR instead of opening a second one. For a task on an existing branch, the recorded PR SHALL be attached.

#### Scenario: Crash after creation
- **WHEN** the PR was created but not recorded and the next push report arrives
- **THEN** the existing PR is found and recorded; still exactly one PR exists

#### Scenario: PR opened by a human in the meantime
- **WHEN** `DF-14` on `fix/login` had no PR at creation, a human opened PR #44 `fix/login → main`, and `DF-14` reports its first push ahead of `main`
- **THEN** PR #44 is recorded as attached and no second PR is opened

### Requirement: Ready for review on completion
When a run ends with outcome `completed` and the task has a PR, the control plane SHALL post the completion summary as a PR comment and, unless the PR is attached, mark it ready for review. The task becomes `in_review`, or `queued` if undelivered messages exist (see "Run end maps to task status"); the PR actions are the same in both cases. Marking a task done by hand SHALL mark a PR that isn't attached ready as well, without a summary.

#### Scenario: Task completed
- **WHEN** run 2 of `DF-1` ends with `completed`
- **THEN** the PR is no longer a draft, carries the summary as a comment, and `DF-1` is `in_review`

#### Scenario: Message waiting at completion
- **WHEN** run 2 of `DF-1` ends with `completed` while a board member's message is still undelivered
- **THEN** the PR is marked ready and gets the summary, and `DF-1` becomes `queued` so the next run picks up the message

#### Scenario: Marked done by hand
- **WHEN** a board member marks `DF-1` with a draft PR done
- **THEN** the PR is no longer a draft and gets no summary comment

#### Scenario: Attached draft PR
- **WHEN** run 1 of `DF-15`, attached to a human's draft PR #45, ends with `completed`
- **THEN** PR #45 gets the summary as a comment and stays a draft

### Requirement: Back to draft when the task reopens
When a message reopens a task in status `in_review` or `completed` whose PR is not merged and not attached, the control plane SHALL mark the PR as draft. A forge failure SHALL be retried with backoff, logged, and SHALL NOT block the run. Further messages before the next completion SHALL NOT change the PR again.

#### Scenario: Review wakes a task in review
- **WHEN** a member's review wakes `DF-1` in status `in_review`
- **THEN** the PR of `DF-1` is a draft again, and the next completion marks it ready

#### Scenario: Human message on a completed task
- **WHEN** a board member queues a message on `DF-2`, which was marked done with a ready PR
- **THEN** the PR of `DF-2` is a draft again

#### Scenario: Merged PR stays merged
- **WHEN** a board member queues a message on `DF-3`, whose PR was merged
- **THEN** the forge is not called

#### Scenario: Closed PR stays closed
- **WHEN** a message reopens `DF-4` in status `in_review`, whose PR was closed without merge
- **THEN** the PR of `DF-4` is not changed

#### Scenario: Task finished again before the draft update
- **WHEN** the forge is unavailable after a message reopened `DF-5`, and `DF-5` reaches `in_review` with a ready PR before the forge is back
- **THEN** the PR of `DF-5` stays ready

#### Scenario: Attached PR stays as it is
- **WHEN** a member's review wakes `DF-13` in status `in_review`, attached to Renovate's PR #40
- **THEN** the forge is not called to change PR #40
