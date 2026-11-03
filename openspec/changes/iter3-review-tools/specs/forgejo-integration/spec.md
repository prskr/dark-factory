# Spec Delta

## ADDED Requirements

### Requirement: List pull request feedback
`list-feedback` SHALL return the pull request's PR comments, its submitted reviews (pending reviews excluded) and its review threads. Review comments of the same review at the same path and line SHALL form one review thread, as Forgejo shows them, its `resolved` flag set when Forgejo reports a resolver. With `since`, it SHALL apply the time filter of the `pull-requests` contract.

#### Scenario: Line comments grouped into a thread
- **WHEN** a reviewer comments on line 12 of `src/lib.rs` and the bot replies in that thread
- **THEN** both review comments are in one review thread with path `src/lib.rs` and line 12

#### Scenario: Two reviews on the same line
- **WHEN** two reviewers each start a review with a comment on line 12 of `src/lib.rs`
- **THEN** `list-feedback` returns two review threads for that line

#### Scenario: Pending review hidden
- **WHEN** a reviewer has started but not submitted a review
- **THEN** neither the review nor its comments are returned

### Requirement: Author roles
Every author SHALL get role `bot` if it is the account that owns the plugin's credential, `member` if its repository permission is write, admin or owner, and `outside` otherwise. Each distinct author SHALL be looked up at most once per call. The lookup needs repository admin rights for the plugin's credential; without them `list-feedback` SHALL fail with `unauthorized`.

#### Scenario: Collaborator with write access
- **WHEN** a collaborator with write permission submitted a review
- **THEN** the review's author role is `member`

### Requirement: Reply in a review thread
`reply` SHALL add a review comment at the thread's path and line to the review that started the thread, so Forgejo shows it in the same thread. A thread id the plugin didn't issue for that pull request SHALL yield `invalid-input`.

#### Scenario: Reply joins the thread
- **WHEN** `reply` is called with a thread id from `list-feedback`
- **THEN** the next `list-feedback` returns the reply as the last review comment of that thread
