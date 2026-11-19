# forgejo-integration Specification

## Purpose

The Forgejo plugin implements pull requests and issues for Forgejo behind the forge-neutral `darkfactory:integrations@0.1.0` interfaces, hiding Forgejo specifics such as its work-in-progress title prefix.

## Requirements

### Requirement: Create pull request
`create` SHALL open a PR from head into base with title and body via the Forgejo API. With `draft: true` it SHALL apply Forgejo's work-in-progress marker; the returned record SHALL have `draft: true` and the title without the marker. An existing open PR for the same head and base SHALL yield `conflict`.

#### Scenario: Draft PR
- **WHEN** `create` is called with title "DF-1: Fix login" and `draft: true`
- **THEN** Forgejo shows a work-in-progress PR and the result has title "DF-1: Fix login" and `draft: true`

#### Scenario: Duplicate
- **WHEN** an open PR for `factory/task/DF-1 → main` exists and `create` is called again
- **THEN** the result is `conflict`

### Requirement: Find open pull request
`find-open` SHALL return the open PR from head into base if one exists, else none.

#### Scenario: Recover after crash
- **WHEN** a PR was created but not recorded
- **THEN** `find-open` returns it with number, URL and head SHA

### Requirement: Update pull request
`update` SHALL change only the given fields; `draft: false` SHALL remove the work-in-progress marker, `draft: true` add it, and a new title SHALL keep the current draft state.

#### Scenario: Ready for review
- **WHEN** `update` is called with `draft: false`
- **THEN** the PR title no longer has the marker and Forgejo no longer treats it as work in progress

#### Scenario: Retitle a draft
- **WHEN** a draft PR gets the new title "DF-1: Fix login redirect"
- **THEN** it remains a draft with that title

### Requirement: Issues
`get` SHALL return an issue's title, body, state, labels and URL; `add-comment` SHALL add a Markdown comment and return its id and URL. PR numbers SHALL be accepted as issue numbers.

#### Scenario: Comment on a PR
- **WHEN** `add-comment` is called with a PR's number
- **THEN** the comment appears on the PR

### Requirement: Error mapping
HTTP 404 SHALL map to `not-found`, 401/403 to `unauthorized`, 409 and "already exists" responses to `conflict`, 422 to `invalid-input`, 429 to `rate-limited` (with `Retry-After` if present), other non-2xx to `upstream`, missing `base-url` to `config`.

#### Scenario: Bad token
- **WHEN** the injected token is invalid
- **THEN** every call returns `unauthorized`

### Requirement: Forgejo webhook signature
`handle-webhook` SHALL hex-decode the `X-Forgejo-Signature` header and check it through the host. A missing or malformed header SHALL fail the check.

#### Scenario: Missing header
- **WHEN** a request has no `X-Forgejo-Signature`
- **THEN** the host answers `401`

### Requirement: Forgejo webhook events
`handle-webhook` SHALL map Forgejo events, by `X-Forgejo-Event` and payload, to the normalized events with the repository URL, the actor's login and, where Forgejo provides it, the head branch: a submitted review, a closed PR, a failed Actions run, and an issue or PR comment with its mentioned logins. Every other event SHALL yield no events.

#### Scenario: Commit status from an external CI
- **WHEN** an external CI sets a commit status with state `failure` on `factory/task/DF-1`
- **THEN** Forgejo 15 sends no webhook, so no task is woken

#### Scenario: Single review comment
- **WHEN** the recorded payload of a single line comment arrives (Forgejo sends it as a comment review without body)
- **THEN** the result is one review submitted event with verdict comment

#### Scenario: Review submitted with changes requested
- **WHEN** the recorded payload of a review requesting changes on the PR from `factory/task/DF-1` arrives
- **THEN** the result is one review submitted event with verdict changes requested, head `factory/task/DF-1` and the reviewer's login

#### Scenario: Merged PR
- **WHEN** the recorded payload of a merged PR arrives
- **THEN** the result is one pull request closed event with merged set

#### Scenario: Mention in a PR comment
- **WHEN** a comment "@factory-bot please rebase" on PR #12 arrives
- **THEN** the result is one comment event for #12, marked as a pull request, with mentioned login `factory-bot`

#### Scenario: Failed Actions run on a push
- **WHEN** the recorded payload of a failed Actions run triggered by a push to `factory/task/DF-3` arrives
- **THEN** the result is one CI failed event for head `factory/task/DF-3` with the workflow file as check name and the run URL

#### Scenario: Failed Actions run on a pull request
- **WHEN** the recorded payload of a failed Actions run triggered by the PR from `factory/task/DF-3` arrives
- **THEN** the result is one CI failed event for head `factory/task/DF-3`, taken from the run's event payload

#### Scenario: Unhandled event
- **WHEN** a `push` event arrives
- **THEN** the result is an empty list

#### Scenario: Pull request from a fork
- **WHEN** a review, close or failed Actions run payload arrives whose PR head repository (`head.repo.id`) differs from the event's repository or is null
- **THEN** the result is an empty list

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

### Requirement: Reply in a review thread
`reply` SHALL add a review comment at the thread's path and line to the review that started the thread, so Forgejo shows it in the same thread. A thread id the plugin didn't issue for that pull request SHALL yield `invalid-input`.

#### Scenario: Reply joins the thread
- **WHEN** `reply` is called with a thread id from `list-feedback`
- **THEN** the next `list-feedback` returns the reply as the last review comment of that thread

### Requirement: Review comments of the newest reviews
`list-feedback` SHALL read the review comments of at most 20 submitted reviews per call, the newest by submission first. When the pull request has more submitted reviews, it SHALL still return every review the review listing returned, but no review threads of the older ones, and SHALL set `truncated: true`. The review listing SHALL stop after 1,000 reviews and then set `truncated: true`.

#### Scenario: Many reviews
- **WHEN** the pull request has 25 submitted reviews, each with one line comment
- **THEN** `list-feedback` returns 25 reviews, the review threads of the 20 newest reviews and `truncated: true`

#### Scenario: Few reviews
- **WHEN** the pull request has 3 submitted reviews
- **THEN** the review threads of all 3 are returned and `truncated` is `false`

### Requirement: List open pull requests by head
`list-open` SHALL return every open pull request of the repository whose head branch in that repository equals the filter's head branch, following Forgejo's pagination. Pull requests from forks SHALL NOT match.

#### Scenario: One open PR
- **WHEN** `list-open` is called with head `renovate/serde-1.x` and PR #40 `renovate/serde-1.x → main` is open
- **THEN** the result contains only PR #40

#### Scenario: Closed PR
- **WHEN** the only PR from `fix/login` is closed
- **THEN** `list-open` with head `fix/login` returns an empty list

#### Scenario: Several bases
- **WHEN** PRs #42 `fix/shared → main` and #43 `fix/shared → release/1.2` are open
- **THEN** `list-open` with head `fix/shared` returns both

### Requirement: List open pull requests with filters
`list-open` SHALL additionally filter by `head-prefix`, `author`, `label` and `base`, all optional and combined with AND. The head prefix SHALL be matched on the plugin side. Filtering by author SHALL require Forgejo v10 or later and SHALL otherwise fail as `unsupported`.

#### Scenario: Prefix and label
- **WHEN** `list-open` is called with head prefix `renovate/` and label `major`, and #40 (`renovate/a`, label `major`) and #41 (`renovate/b`, no label) are open
- **THEN** the result contains only #40

### Requirement: CI status from commit statuses
`ci.status` SHALL read the combined commit status of the ref and return one check per status context with its state and target URL. For a check created by Forgejo Actions on v16 or later, it SHALL include the job id resolved through the Actions API.

#### Scenario: Actions job failed
- **WHEN** the head of PR #40 has status "ci / test (pull_request)" with state `failure` from Forgejo Actions on v16
- **THEN** `ci.status` returns state `failure` and that check with its URL and job id

#### Scenario: Older Forgejo
- **WHEN** the same status exists on Forgejo v15
- **THEN** the check is returned with state and URL and without a job id

### Requirement: CI job logs
`ci.job-log` SHALL return the plain-text log of the given Actions job, at most its last 4 MiB, through the Actions API of Forgejo v16 or later. On an older Forgejo it SHALL fail as `unsupported` with a message naming v16.

#### Scenario: Log read
- **WHEN** `ci.job-log` is called for job 812 on Forgejo v16
- **THEN** the job's plain-text log is returned

#### Scenario: Forgejo v15
- **WHEN** `ci.job-log` is called on Forgejo v15
- **THEN** the call fails as `unsupported` naming Forgejo v16

### Requirement: Merge a pull request
`merge` SHALL merge an open pull request only if its head is still the given head SHA, using the repository's merge style: the repository's default merge style. A moved head, a merge conflict or a failed required check SHALL be returned as `conflict` or `invalid-input` with Forgejo's message; an already merged pull request with the same head SHALL count as success.

#### Scenario: Merged
- **WHEN** `merge` is called for open PR #12 with its current head SHA
- **THEN** PR #12 is merged and the call succeeds

#### Scenario: Head moved
- **WHEN** `merge` is called with a head SHA that is no longer PR #12's head
- **THEN** the call fails and PR #12 stays open

#### Scenario: Already merged
- **WHEN** `merge` is called again for PR #12 after it was merged with that head
- **THEN** the call succeeds

### Requirement: Create a branch
`create-branch` SHALL create the branch `name` at the tip of the branch `from`. If `name` already exists, it SHALL return `conflict` and leave the branch unchanged; an unknown `from` SHALL return `not-found` or `invalid-input`.

#### Scenario: Feature branch
- **WHEN** `create-branch` is called with name `factory/task/DF-30` and from `main`
- **THEN** `factory/task/DF-30` exists at the tip of `main`

#### Scenario: Branch exists
- **WHEN** `create-branch` is called for an existing `factory/task/DF-30`
- **THEN** the call fails with `conflict` and the branch is unchanged
