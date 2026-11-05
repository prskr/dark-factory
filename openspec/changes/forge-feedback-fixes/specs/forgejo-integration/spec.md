# Spec Delta

## MODIFIED Requirements

### Requirement: Forgejo bot login and author role
`whoami` SHALL return the login of the account owning the plugin's credential. `author-role` SHALL return `bot` for that account, `member` for write, admin or owner permission on the repository, and `outside` otherwise, including when Forgejo refuses the permission lookup; then the plugin SHALL write the same warning as `list-feedback`.

#### Scenario: Reader
- **WHEN** `author-role` is called for a user with read permission
- **THEN** the result is `outside`

#### Scenario: Lookup refused
- **WHEN** the plugin's account is not repository admin and `author-role` is called for another user
- **THEN** the result is `outside` and the control plane log has a warning naming that user and repository admin

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

#### Scenario: Each PR comment once
- **WHEN** the pull request has three PR comments
- **THEN** `list-feedback` returns exactly those three PR comments and `truncated: false`

#### Scenario: Review updated-at
- **WHEN** a reviewer edits the body of a submitted review
- **THEN** the review's `updated-at` is the time of the edit and its `submitted-at` is unchanged

### Requirement: Author roles
Every author SHALL get role `bot` if it is the account that owns the plugin's credential, `member` if its repository permission is write, admin or owner, and `outside` otherwise. Each distinct author SHALL be looked up at most once per call. When Forgejo refuses the permission lookup (the plugin's account is not repository admin), the author SHALL get role `outside` and the plugin SHALL write a warning that names the author and the missing repository admin access; the call SHALL NOT fail.

#### Scenario: Collaborator with write access
- **WHEN** a collaborator with write permission submitted a review
- **THEN** the review's author role is `member`

#### Scenario: Bot account without repository admin
- **WHEN** the plugin's account has write access only and a collaborator with write permission submitted a review
- **THEN** `list-feedback` succeeds, the review's author role is `outside` and the control plane log has a warning naming the author and repository admin

## ADDED Requirements

### Requirement: Review comments of the newest reviews
`list-feedback` SHALL read the review comments of at most 20 submitted reviews per call, the newest by submission first. When the pull request has more submitted reviews, it SHALL still return every review the review listing returned, but no review threads of the older ones, and SHALL set `truncated: true`. The review listing SHALL stop after 1,000 reviews and then set `truncated: true`.

#### Scenario: Many reviews
- **WHEN** the pull request has 25 submitted reviews, each with one line comment
- **THEN** `list-feedback` returns 25 reviews, the review threads of the 20 newest reviews and `truncated: true`

#### Scenario: Few reviews
- **WHEN** the pull request has 3 submitted reviews
- **THEN** the review threads of all 3 are returned and `truncated` is `false`
