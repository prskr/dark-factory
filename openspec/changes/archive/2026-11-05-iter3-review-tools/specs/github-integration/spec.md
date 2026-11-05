# Spec Delta

## ADDED Requirements

### Requirement: List pull request feedback
`list-feedback` SHALL return the pull request's PR comments, its submitted reviews (pending reviews excluded) and its review threads with GitHub's resolved state. With `since`, it SHALL apply the time filter of the `pull-requests` contract.

#### Scenario: Resolved thread
- **WHEN** a reviewer resolved a review thread
- **THEN** that thread is returned with `resolved: true`

#### Scenario: Replies kept in their thread
- **WHEN** a reviewer comments on a line and the bot replies to that comment
- **THEN** both review comments are in one review thread

### Requirement: Author roles
Every author SHALL get role `bot` if it is the account that owns the plugin's credential, `member` if GitHub reports its association as owner, member or collaborator and its repository permission is write or admin, and `outside` otherwise, including a deleted author.

#### Scenario: First-time contributor
- **WHEN** a user with association `FIRST_TIME_CONTRIBUTOR` comments
- **THEN** the comment's author role is `outside`

#### Scenario: Collaborator without write access
- **WHEN** a user with association `MEMBER` and repository permission `read` reviews
- **THEN** the review's author role is `outside`

### Requirement: Reply in a review thread
`reply` SHALL add a reply to the review thread on GitHub. A thread id the plugin didn't issue for that pull request SHALL yield `invalid-input` or `not-found`.

#### Scenario: Reply joins the thread
- **WHEN** `reply` is called with a thread id from `list-feedback`
- **THEN** the next `list-feedback` returns the reply as the last review comment of that thread
