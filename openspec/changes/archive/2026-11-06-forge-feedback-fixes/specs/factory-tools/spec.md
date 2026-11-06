# Spec Delta

## MODIFIED Requirements

### Requirement: Integration failures are tool errors
Forge errors SHALL be returned as `is_error: true` results with a message the model can act on (e.g. rate limited, retry later); they SHALL NOT end the run. A refused request (`unauthorized`) SHALL show the forge's message and the request that failed, and SHALL ask the agent to have a human check the bot account's access and the project's forge credential.

#### Scenario: Forge unreachable
- **WHEN** Forgejo is down during `comment_on_issue`
- **THEN** the result has `is_error: true` and the run continues

#### Scenario: Refused request
- **WHEN** Forgejo answers a request of `read_pull_request_feedback` with 403 and message "token does not have required scope"
- **THEN** the result has `is_error: true` and its text contains that message and the request path

### Requirement: read_pull_request_feedback
`read_pull_request_feedback` SHALL return the PR comments, reviews (verdict, body) and review threads (thread id, path, line, resolved flag, review comments) of the task's pull request. Every item SHALL carry its author's login and role (`bot`, `member`, `outside`), and no item SHALL be left out because of its author or resolved state. Each of the three lists SHALL be ordered by last activity, newest first.

#### Scenario: Review with line comments
- **WHEN** a member submitted a review "request changes" with two line comments on `src/lib.rs` and the agent calls `read_pull_request_feedback`
- **THEN** the result contains the review with verdict `request_changes` and two review threads on `src/lib.rs`, each with a thread id and author role `member`

#### Scenario: Own and resolved items included
- **WHEN** the PR has a PR comment by the bot account and a resolved review thread
- **THEN** both are in the result, the comment with role `bot` and the thread with `resolved: true`

#### Scenario: Outside author labelled
- **WHEN** a user without write access comments on the PR
- **THEN** the comment is in the result with role `outside`

#### Scenario: Newest first
- **WHEN** PR comment A was written yesterday, PR comment B this morning, and comment A was edited an hour ago
- **THEN** `comments` lists A before B

### Requirement: Feedback since a point in time
With `since` (RFC 3339 timestamp), `read_pull_request_feedback` SHALL return only PR comments and reviews created or updated at or after `since`, and review threads with at least one such review comment, each such thread with all of its review comments, except threads the forge plugin did not read because of a listing limit (then `truncated` is `true`). Every result SHALL carry `next_since`: the later of `since` and the latest parsable activity of the returned items minus 5 minutes; without such an item, the given `since`, or null. The control plane SHALL NOT store what an agent has read.

#### Scenario: Only new feedback
- **WHEN** a PR comment was written yesterday, a review today, and the agent passes `since` = this morning
- **THEN** the result contains the review and not the PR comment

#### Scenario: Thread context kept
- **WHEN** a reply was added today to a review thread started last week and the agent passes `since` = this morning
- **THEN** the thread is in the result with both review comments

#### Scenario: Cursor for the next read
- **WHEN** the agent passes no `since` and the newest item in the result is a review comment updated at `2026-11-05T10:15:00Z`
- **THEN** `next_since` is `2026-11-05T10:10:00Z`, and passing it as `since` returns that item again and everything at or after `10:10:00Z`

#### Scenario: Item created between two listings
- **WHEN** a PR comment is created at `10:14:58Z` after the plugin listed PR comments and before it listed reviews, and the newest returned review was submitted at `10:15:00Z`
- **THEN** the PR comment is not in this result, `next_since` is `10:10:00Z`, and the next read with that `since` returns the PR comment

#### Scenario: Cursor does not move back
- **WHEN** the agent passes `since` = `2026-11-05T10:15:00Z` and the only returned item is from `10:16:00Z`
- **THEN** `next_since` is `2026-11-05T10:15:00Z`

#### Scenario: Nothing new
- **WHEN** the agent passes `since` = `2026-11-05T10:15:00Z` and no item has activity at or after it
- **THEN** the lists are empty and `next_since` is `2026-11-05T10:15:00Z`

#### Scenario: No parsable timestamp
- **WHEN** no `since` is given and every returned item has an empty or malformed timestamp
- **THEN** `next_since` is null

#### Scenario: Older review thread beyond the listing limit
- **WHEN** a reply is added today to a thread of a Forgejo review that has 20 newer reviews, and the agent passes `since` = this morning
- **THEN** the thread is not in the result and `truncated` is `true`

### Requirement: Feedback size limit
`read_pull_request_feedback` SHALL keep its result within a fixed size budget by leaving out whole items, oldest last activity first, and SHALL then set `truncated: true`; otherwise `truncated` SHALL be `false`. A review's last activity SHALL be the later of its `submitted-at` and `updated-at`. It SHALL NOT offer pagination.

#### Scenario: Large pull request
- **WHEN** the PR's feedback exceeds the budget
- **THEN** the result holds the most recently active items within the budget and `truncated: true`

#### Scenario: Edited old review kept
- **WHEN** the feedback exceeds the budget, a review was submitted last week and its body edited today, and several PR comments are from yesterday
- **THEN** the review is in the result and the PR comments are left out first

## ADDED Requirements

### Requirement: Feedback tool description guides incremental reads
The description of `read_pull_request_feedback` SHALL tell the agent to pass the `next_since` of its previous read as `since` and to skip ids it already handled, that lists are newest first, that `truncated` means older items were left out that later reads with `next_since` do not return, and that role `outside` can also mean the forge refused the role lookup.

#### Scenario: Description content
- **WHEN** the tool set is listed
- **THEN** the description of `read_pull_request_feedback` mentions `next_since`, repeated ids, newest first, `truncated` with the left-out items, and the refused role lookup
