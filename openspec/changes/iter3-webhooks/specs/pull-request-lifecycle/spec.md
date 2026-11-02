# Spec Delta

## ADDED Requirements

### Requirement: PR closed without merge
When the forge reports the task's PR as closed without merge, the control plane SHALL wake the task as for other webhook events, with a message naming the PR and the login that closed it. The task's status SHALL NOT change otherwise.

#### Scenario: Closed by a maintainer
- **WHEN** `bob` closes the PR of `DF-1` in status `in_review` without merging
- **THEN** `DF-1` gets a message that `bob` closed PR #12 without merge and becomes `queued`

### Requirement: Back to draft when the task reopens
When a message reopens a task in status `in_review` or `completed` whose PR is not merged, the control plane SHALL mark the PR as draft. A forge failure SHALL be retried with backoff, logged, and SHALL NOT block the run. Further messages before the next completion SHALL NOT change the PR again.

#### Scenario: Review wakes a task in review
- **WHEN** a member's review wakes `DF-1` in status `in_review`
- **THEN** the PR of `DF-1` is a draft again, and the next completion marks it ready

#### Scenario: Human message on a completed task
- **WHEN** a board member queues a message on `DF-2`, which was marked done with a ready PR
- **THEN** the PR of `DF-2` is a draft again

#### Scenario: Merged PR stays merged
- **WHEN** a board member queues a message on `DF-3`, whose PR was merged
- **THEN** the forge is not called
