# Spec Delta

## Purpose

Lets forge events (submitted reviews, closed or merged pull requests, CI failures and mentions) reach the right task without a human, through a signed webhook endpoint, deterministic filtering and routing in the control plane, and a debounced wake-up message (ADR-0015).

## ADDED Requirements

### Requirement: Webhook endpoint
The control plane SHALL accept `POST /webhooks/{plugin}` without login, session or CSRF token. It SHALL answer `404` when the plugin is unknown, not loaded or has no webhook secret, `413` for a body over 1 MiB, `401` when the signature check fails, `400` when the plugin rejects the payload, and `202` once the events are processed, including when none applies. Every response body SHALL be empty.

#### Scenario: Valid delivery
- **WHEN** Forgejo posts a signed review event to `/webhooks/forgejo`
- **THEN** the response is `202` with an empty body

#### Scenario: No secret configured
- **WHEN** `plugins.github` has no `webhookSecret` and a request arrives at `/webhooks/github`
- **THEN** the response is `404` and the plugin is not called

#### Scenario: Oversized body
- **WHEN** a request to `/webhooks/forgejo` has a 2 MiB body
- **THEN** the response is `413` and the plugin is not called

#### Scenario: Processing error
- **WHEN** the database is unavailable while an accepted event is stored
- **THEN** the response is `500`, so the forge redelivers

### Requirement: Signature verification
A request SHALL be accepted only if the plugin verified its signature through the host, and the host found it to be the HMAC-SHA256 of the raw body with the plugin's webhook secret, compared in constant time. If the plugin returns events without a successful verification, the host SHALL answer `401` and discard the events.

#### Scenario: Wrong secret
- **WHEN** a request is signed with another secret
- **THEN** the response is `401` and nothing is stored

#### Scenario: Plugin skips verification
- **WHEN** a plugin returns events without calling the verification
- **THEN** the response is `401` and the events are discarded

### Requirement: Accepted events
The control plane SHALL act on four normalized events and nothing else: review submitted (with verdict), pull request closed (merged or not), CI failed, and comment with mentioned logins. A forge event of any other kind, including a single review comment outside a submitted review, SHALL be answered with `202` and ignored.

#### Scenario: Single review comment
- **WHEN** a reviewer adds a line comment to a pending review
- **THEN** no task is woken

#### Scenario: Push event
- **WHEN** the forge posts a push event
- **THEN** the response is `202` and nothing is stored

### Requirement: Repository match
An event SHALL only apply to tasks of a project whose repository URL matches the event's repository, compared by host, owner and name, case-insensitively and without a trailing `.git`.

#### Scenario: Branch name of another repository
- **WHEN** a signed review event from `git.example.org/eve/other` names head branch `factory/task/DF-1`, and project `DF` is on `git.example.org/peter/dark-factory`
- **THEN** `DF-1` is not woken

### Requirement: Routing
Review, close and CI events SHALL route to the task whose key the head branch `factory/task/{key}` names. A comment on a pull request SHALL route to the project's task with that PR number. A comment on an issue SHALL route to every non-final task of the project linked to that issue. An event matching no task SHALL be logged at debug level and dropped.

#### Scenario: Review on a task PR
- **WHEN** a member submits a review on the PR from `factory/task/DF-1`
- **THEN** the event is routed to `DF-1`

#### Scenario: Mention on a linked issue
- **WHEN** a member mentions the bot on issue #7, and `DF-3` and `DF-5` are non-final tasks linked to #7
- **THEN** the event is routed to both

#### Scenario: Branch outside the factory
- **WHEN** a review is submitted on a PR from `feature/login`
- **THEN** no task is woken and a debug event is logged

### Requirement: Bot authored events dropped
Reviews and comments whose author is the account that owns the project's forge credential SHALL be dropped. Closed, merged and CI failure events SHALL apply whoever the actor is.

#### Scenario: Agent replies on its own PR
- **WHEN** the agent of `DF-1` posts a PR comment through the bot account
- **THEN** `DF-1` is not woken

#### Scenario: Bot merges a PR
- **WHEN** the bot account merges the PR of `DF-4`
- **THEN** `DF-4` is completed

### Requirement: Wake policy per project
Reviews and mentions from authors other than the bot SHALL pass the project's `webhook_wake` setting: `none` drops them, `members` accepts only authors with write access or more, and `anyone` accepts every author. A comment SHALL count as a mention only if it mentions the bot account.

#### Scenario: Outside review with members policy
- **WHEN** a user without write access submits a review on the PR of `DF-1` and the project's setting is `members`
- **THEN** `DF-1` is not woken

#### Scenario: Outside review with anyone policy
- **WHEN** the same review arrives and the setting is `anyone`
- **THEN** the review is accepted for `DF-1`

#### Scenario: Merge with none policy
- **WHEN** the PR of `DF-1` is merged and the setting is `none`
- **THEN** `DF-1` is completed

#### Scenario: Comment without mention
- **WHEN** a member comments on the PR of `DF-1` without mentioning the bot
- **THEN** `DF-1` is not woken

### Requirement: Debounced wake-up
Accepted review, CI failure, mention and closed-without-merge events SHALL be collected per task. 30 s after the first collected event, the control plane SHALL queue one message from the system principal (reason `webhook`) that lists every event collected until then, and SHALL forget the collected events.

#### Scenario: Review and CI failure together
- **WHEN** a review is submitted on the PR of `DF-1` and 10 s later CI fails on its branch
- **THEN** about 30 s after the review, `DF-1` has one new message naming both events

#### Scenario: Event after the window
- **WHEN** a second review arrives 40 s after the first
- **THEN** it is delivered in a second message 30 s later

### Requirement: Only non-final tasks wake
Debounced events SHALL wake a task in any status except `completed`, `cancelled` and `failed`. Events collected for a task that is in one of those statuses when the window ends SHALL be dropped and logged at debug level.

#### Scenario: Review after merge
- **WHEN** a review is submitted on the merged PR of completed task `DF-1`
- **THEN** `DF-1` stays `completed` and no message is queued

#### Scenario: Waiting task
- **WHEN** CI fails on the branch of `DF-2` in status `waiting`
- **THEN** a message is queued and `DF-2` becomes `queued`

### Requirement: Message content
The wake-up message SHALL name, per event, its kind, the actor's login, the review verdict or the CI check name, and the PR or issue number, and SHALL point to `read_pull_request_feedback` for reviews and comments. It SHALL NOT contain review, comment or CI log text from the payload.

#### Scenario: Review message
- **WHEN** `alice` submits a review requesting changes on PR #12 of `DF-1`
- **THEN** the message says that `alice` requested changes on PR #12 and to read the feedback with `read_pull_request_feedback`, and contains no part of the review body

### Requirement: Webhook metric
The control plane SHALL count webhook requests by plugin and outcome (`accepted`, `unauthorized`, `not_found`, `too_large`, `invalid`, `error`).

#### Scenario: Rejected signature counted
- **WHEN** a request to `/webhooks/forgejo` fails the signature check
- **THEN** the counter for plugin `forgejo` and outcome `unauthorized` increases by one
