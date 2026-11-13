# forge-webhooks Specification

## Purpose
Lets forge events (submitted reviews, closed or merged pull requests, CI failures and mentions) reach the right task without a human, through a signed webhook endpoint, deterministic filtering and routing in the control plane, and a wake-up message per event (ADR-0015).

## Requirements

### Requirement: Webhook endpoint
The control plane SHALL accept `POST /webhooks/{plugin}` without login, session or CSRF token. It SHALL answer `404` when the plugin is unknown, not loaded, exports no `webhooks` interface or has no `webhookSecret` in the instance config (the Helm chart always sets one), `413` for a body over 1 MiB, `401` when the signature check fails, `400` when the plugin rejects the payload, and `202` once the events are processed, including when none applies. Every response body SHALL be empty.

#### Scenario: Valid delivery
- **WHEN** Forgejo posts a signed review event to `/webhooks/forgejo`
- **THEN** the response is `202` with an empty body

#### Scenario: Plugin without webhook support
- **WHEN** a loaded plugin exports no `webhooks` interface and a request arrives at its `/webhooks/<name>`
- **THEN** the response is `404` and the plugin is not called

#### Scenario: No secret configured
- **WHEN** a hand-written instance config has no `plugins.github.webhookSecret` and a request arrives at `/webhooks/github`
- **THEN** the response is `404` and the plugin is not called

#### Scenario: Forge not yet configured
- **WHEN** the operator has not entered the webhook secret in the forge and an unsigned request arrives at `/webhooks/github`
- **THEN** the response is `401` and nothing is stored

#### Scenario: Oversized body
- **WHEN** a request to `/webhooks/forgejo` has a 2 MiB body
- **THEN** the response is `413` and the plugin is not called

#### Scenario: Processing error
- **WHEN** the database is unavailable while an accepted event is stored
- **THEN** the response is `500` and the forge records a failed delivery, which an operator can redeliver

### Requirement: Signature verification
A request SHALL be accepted only if the plugin verified its signature through the host, and the host found it to be the HMAC-SHA256 of the raw body with the plugin's webhook secret, compared in constant time. If the plugin returns events without a successful verification, the host SHALL answer `401` and discard the events.

#### Scenario: Wrong secret
- **WHEN** a request is signed with another secret
- **THEN** the response is `401` and nothing is stored

#### Scenario: Plugin skips verification
- **WHEN** a plugin returns events without calling the verification
- **THEN** the response is `401` and the events are discarded

### Requirement: Accepted events
The control plane SHALL act on four normalized events and nothing else: review submitted (with verdict), pull request closed (merged or not), CI failed, and comment with mentioned logins. A single review comment that the forge publishes at once counts as a submitted review with verdict comment. A forge event of any other kind, including a line comment added to a pending review, SHALL be answered with `202` and ignored.

#### Scenario: Comment in a pending review
- **WHEN** a reviewer adds a line comment to a pending review
- **THEN** no task is woken

#### Scenario: Single review comment
- **WHEN** a member posts a single line comment on the PR of `DF-1`
- **THEN** the comment is accepted as a submitted review with verdict comment

#### Scenario: Push event
- **WHEN** the forge posts a push event
- **THEN** the response is `202` and nothing is stored

### Requirement: Repository match
An event SHALL only apply to tasks of a project whose repository URL matches the event's repository, compared by host, owner and name, case-insensitively and without a trailing `.git`. A plugin SHALL yield no review, close or CI failure event for a pull request whose head branch lives in another repository than the event's (a fork, or a deleted fork), because a fork's branch can carry a task branch name.

#### Scenario: Branch name of another repository
- **WHEN** a signed review event from `git.example.org/eve/other` names head branch `factory/task/DF-1`, and project `DF` is on `git.example.org/peter/dark-factory`
- **THEN** `DF-1` is not woken

#### Scenario: Pull request from a fork
- **WHEN** a pull request on `git.example.org/peter/dark-factory` from branch `factory/task/DF-1` of the fork `git.example.org/eve/dark-factory` is reviewed, closed or fails CI
- **THEN** `DF-1` is not woken and its PR is not marked draft

### Requirement: Routing
Review, close and CI events SHALL route to the task of the project that holds the event's head branch (see `task-branches`, "One open task per branch"), or, if no open task holds it, to the most recently created task with that branch. A comment on a pull request SHALL route to the project's task with that PR number. A comment on an issue SHALL route to every non-final task of the project linked to that issue. An event matching no task SHALL be logged at debug level and dropped.

#### Scenario: Review on a task PR
- **WHEN** a member submits a review on the PR from `factory/task/DF-1`
- **THEN** the event is routed to `DF-1`

#### Scenario: Review on an attached PR
- **WHEN** a member submits a review on Renovate's PR #40 from `renovate/serde-1.x`, which `DF-13` holds
- **THEN** the event is routed to `DF-13`

#### Scenario: Mention on a linked issue
- **WHEN** a member mentions the bot on issue #7, and `DF-3` and `DF-5` are non-final tasks linked to #7
- **THEN** the event is routed to both

#### Scenario: Branch outside the factory
- **WHEN** a review is submitted on a PR from `feature/login`, which no task has
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
Reviews and mentions from authors other than the bot SHALL pass the project's `webhook_wake` setting: `none` drops them, `members` accepts only authors with write access or more, and `anyone` accepts every author. A comment SHALL count as a mention only if it mentions the bot account. With `members`, an author whose role the forge refuses to look up SHALL count as not a member; the event SHALL be dropped, not fail.

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

#### Scenario: Role lookup refused with members policy
- **WHEN** a member submits a review on the PR of `DF-1`, the setting is `members`, and the Forgejo bot account is not repository admin
- **THEN** `DF-1` is not woken, the event is acknowledged, and the control plane log has the plugin's warning naming the author

### Requirement: Wake-up message per event
Every accepted review, CI failure, mention and closed-without-merge event SHALL queue one message from the system principal (reason `webhook`) on each task it routes to, in the same transaction that processes the event. Like other queued messages, messages arriving during a turn SHALL be held back and forwarded after it, so a burst of events wakes the task once and never starts a second run.

#### Scenario: Review and CI failure together
- **WHEN** a review is submitted on the PR of `DF-1` in status `in_review` and 10 s later CI fails on its branch
- **THEN** `DF-1` has two new messages, becomes `queued` once, and its PR is marked draft once

### Requirement: Only non-final tasks wake
Events SHALL wake a task in any status except `completed`, `cancelled` and `failed`. An event for a task in one of those statuses SHALL be dropped and logged at debug level.

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
The control plane SHALL count webhook requests by plugin and outcome (`accepted`, `unauthorized`, `not_found`, `too_large`, `invalid`, `error`). A request for a loaded plugin SHALL be counted under the plugin's name, a request for any other name under `unknown`, so the label set stays bounded by the configuration.

#### Scenario: Rejected signature counted
- **WHEN** a request to `/webhooks/forgejo` fails the signature check
- **THEN** the counter for plugin `forgejo` and outcome `unauthorized` increases by one

#### Scenario: Loaded plugin without secret counted
- **WHEN** a hand-written instance config loads `plugins.github` without `webhookSecret` and a request arrives at `/webhooks/github`
- **THEN** the counter for plugin `github` and outcome `not_found` increases by one

#### Scenario: Loaded plugin without webhook support counted
- **WHEN** a loaded plugin `example` exports no `webhooks` interface and a request arrives at `/webhooks/example`
- **THEN** the counter for plugin `example` and outcome `not_found` increases by one

### Requirement: Webhook secret source
At startup the control plane SHALL read the webhook secret of every loaded plugin that exports `webhooks` and has `webhookSecret` in the instance config from a key of a Kubernetes Secret in the instance namespace. `webhookSecret` names the Secret, the key and whether the control plane may generate it.

#### Scenario: Existing Secret
- **WHEN** `plugins.forgejo` references Secret `forge-webhooks`, key `forgejo`, without generation, and that key holds `s3cr3t`
- **THEN** a request to `/webhooks/forgejo` signed with `s3cr3t` is accepted

#### Scenario: Plugin without webhook support needs no secret
- **WHEN** a loaded plugin exports no `webhooks` interface
- **THEN** the control plane reads and creates no webhook secret for it

### Requirement: Generated webhook secret
When generation is allowed and the Secret does not exist, the control plane SHALL create it with 32 random bytes, hex-encoded, under the configured key, labelled with the instance and the plugin and without an owner. If another replica created it first, the control plane SHALL read that Secret instead. It SHALL never change an existing Secret.

#### Scenario: First start
- **WHEN** the control plane starts with generation allowed for `forgejo` and Secret `df-webhook-forgejo` does not exist
- **THEN** it creates `df-webhook-forgejo` with key `secret` holding a 64-character hex value, labelled `app.kubernetes.io/managed-by: dark-factory` and `dark-factory.io/plugin: forgejo`, and uses that value

#### Scenario: Restart keeps the secret
- **WHEN** the control plane restarts and `df-webhook-forgejo` exists
- **THEN** it uses the stored value and does not change the Secret

#### Scenario: Two replicas start together
- **WHEN** two replicas start, neither finds the Secret, and the second one's create fails because the Secret already exists
- **THEN** the second replica reads the Secret and both use the same value

#### Scenario: Generated Secret exists without the key
- **WHEN** generation is allowed and `df-webhook-forgejo` exists but has no key `secret`
- **THEN** startup fails naming the Secret and the key, and the Secret is not changed

### Requirement: Webhook secret failures stop startup
The control plane SHALL fail to start, naming the plugin, the Secret and the key, when a webhook secret cannot be read or created: a Secret or key that is missing while generation is not allowed, an empty value, a denied request or any other Kubernetes API error.

#### Scenario: Existing Secret missing
- **WHEN** `plugins.forgejo` references Secret `forge-webhooks` without generation and that Secret does not exist
- **THEN** startup fails naming `plugins.forgejo`, `forge-webhooks` and the key, and no Secret is created

#### Scenario: Create denied
- **WHEN** generation is allowed but the control plane may not create Secrets
- **THEN** startup fails naming `plugins.forgejo` and the denied request

### Requirement: Webhook secret rotation
The control plane SHALL read webhook secrets only at startup. A changed or deleted Secret SHALL take effect at the next start; a deleted generated Secret SHALL be generated again with a new value.

#### Scenario: Rotation by deletion
- **WHEN** the operator deletes `df-webhook-forgejo` and restarts the control plane
- **THEN** the control plane generates a new value, and requests signed with the old value get `401`

#### Scenario: Deliveries during a rotation
- **WHEN** a rotation's rollout is in progress and old and restarted replicas hold different values
- **THEN** a delivery signed with the value a replica does not hold gets `401` from that replica, and the forge records it as failed so the operator can redeliver it once the rollout and the forge update are done

#### Scenario: Secret changed while running
- **WHEN** the operator changes the value of a referenced Secret without restarting
- **THEN** the control plane keeps checking signatures against the value it read at startup

### Requirement: Incoming webhook URL in project settings
The "Incoming webhook" group of a project's "Integrations" tab SHALL show the webhook URL `<publicUrl>/webhooks/<plugin>` of the forge plugin that serves the project (as in `configuration-management` "Forge of a project"), with a "Copy webhook URL" button. Without `publicUrl` it SHALL show only the path and a hint naming `publicUrl`. The group SHALL say that the webhook secret is shared by every project that plugin serves.

#### Scenario: Webhook URL of a Forgejo project
- **WHEN** an admin opens the "Integrations" tab of project `DF`, served by `forgejo`, on an instance with `publicUrl: https://factory.example.org`
- **THEN** the group shows `https://factory.example.org/webhooks/forgejo` with a "Copy webhook URL" button, and the secret masked

#### Scenario: Without publicUrl
- **WHEN** the instance has no `publicUrl` and an admin opens the tab of `DF`
- **THEN** the group shows `/webhooks/forgejo` and a hint that `publicUrl` makes it a full URL

### Requirement: Masked webhook secret
The group SHALL show the webhook secret masked with a placeholder of fixed length, and the rendered page SHALL NOT contain the secret. An eye button ("Show secret" / "Hide secret", `aria-pressed`) SHALL show or hide it, hiding removes it from the page. A clipboard button ("Copy secret") SHALL copy it without showing it. A copy SHALL be confirmed visibly and to screen readers, and a failed fetch or copy SHALL show an error at the field.

#### Scenario: Secret not in the page
- **WHEN** the "Integrations" tab of `DF` is rendered
- **THEN** the response body does not contain the webhook secret of `forgejo`

#### Scenario: Show and hide the secret
- **WHEN** the admin activates "Show secret"
- **THEN** the secret is shown in place of the placeholder, the button reads "Hide secret" with `aria-pressed="true"`, and activating it again masks the secret and removes it from the page

#### Scenario: Copy without showing
- **WHEN** the admin activates "Copy secret" while the secret is masked
- **THEN** the clipboard holds the secret, the secret stays masked, and the button confirms the copy

### Requirement: Copy buttons need a secure context
The copy buttons for the webhook URL and the secret SHALL be shown only where the browser offers the asynchronous clipboard API (a secure context: HTTPS or a loopback host). Without it, the group SHALL omit both copy buttons and say that copying needs HTTPS; "Show secret" stays available.

#### Scenario: Plain HTTP on a non-loopback host
- **WHEN** an admin opens the "Integrations" tab of `DF` on an instance served at `http://factory.internal`
- **THEN** the group shows no copy buttons, says that copying needs HTTPS, and the secret can still be shown

### Requirement: Webhook secret endpoint
"Show secret" and "Copy secret" SHALL fetch the secret on demand from `GET /projects/{key}/webhook-secret`. The endpoint SHALL require `admin`, answer `text/plain` with `Cache-Control: no-store`, and write an audit entry with actor, project and plugin, without the secret, per successful fetch.

#### Scenario: Reading the secret is audited
- **WHEN** an admin fetches `/projects/DF/webhook-secret`
- **THEN** the response is the secret as `text/plain` with `Cache-Control: no-store`, and the audit log records the read with actor, `DF` and `forgejo` and without the secret

#### Scenario: Secret endpoint without admin
- **WHEN** a user with only `board-member` requests `/projects/DF/webhook-secret`
- **THEN** the response is `403` and nothing is audited

### Requirement: Incoming webhook unavailable
When no webhook URL applies to a project, the group SHALL say why instead of showing URL and secret, and the secret endpoint SHALL answer `404`. The reasons SHALL be: no plugin serves the project, the project has no forge credential, the serving plugin did not load, it has no `webhooks` export, or it has no webhook secret.

#### Scenario: No forge integration
- **WHEN** an admin opens the "Integrations" tab of project `WEB`, which has no forge credential
- **THEN** the group says that no forge integration serves the project, shows no URL and no secret controls, and `/projects/WEB/webhook-secret` answers `404`

#### Scenario: Plugin without webhook support
- **WHEN** the plugin serving a project exports no `webhooks` interface
- **THEN** the group says that this forge integration does not support webhooks
