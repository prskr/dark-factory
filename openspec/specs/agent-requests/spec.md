# agent-requests Specification

## Purpose

One model for everything an agent needs from a human: questions with free-text answers and typed approval requests whose action the control plane applies, with one audit trail (ADR-0038).

## Requirements

### Requirement: Request kinds
A request SHALL be either a question (text, up to 5 suggested answers) or an approval request (type from the closed list of registered types, typed payload, reason). Each request SHALL belong to one task, record who raised it (agent run or control plane) and whether it is blocking.

#### Scenario: Question stored
- **WHEN** the agent of run R on `DF-7` asks "Use Postgres 16 or 17?" with suggested answers "16" and "17"
- **THEN** a pending question for `DF-7` raised by R with both suggested answers exists

#### Scenario: Unknown approval type
- **WHEN** an approval request names type `delete-everything`
- **THEN** it is rejected and nothing is stored

### Requirement: Request states
A request SHALL be `pending` until decided or closed. Questions SHALL become `answered`; approval requests `approved` or `denied`; any pending request MAY become `closed` with a reason. Decided and closed requests SHALL NOT change again.

#### Scenario: Second decision
- **WHEN** two board members decide the same pending request at the same time
- **THEN** exactly one decision is stored and the other receives "already decided"

### Requirement: Identical pending requests are merged
Raising a request whose kind, type and type-defined fingerprint equal a pending request of the same task SHALL return the existing request instead of creating a new one.

#### Scenario: Agent asks twice
- **WHEN** the agent raises the same `network-access` request for `pypi.org:443` twice while the first is pending
- **THEN** one pending request exists and both calls return its id

### Requirement: Limit of pending requests
A task SHALL have at most 20 pending requests; raising more SHALL fail with an error the agent can act on.

#### Scenario: Request flood
- **WHEN** an agent raises a 21st distinct pending request on one task
- **THEN** the call fails with "too many pending requests" and nothing is stored

### Requirement: Decisions
Deciding SHALL require a human with `approval:decide` (iteration 2: the `board-member` role). An answer SHALL carry text. An approval SHALL carry one of the scopes the type offers. A denial MAY carry a comment.

#### Scenario: Answer by suggested answer
- **WHEN** a board member picks the suggested answer "17"
- **THEN** the question is `answered` with text "17" and the deciding user

### Requirement: Approved actions are applied by the control plane
On approval, the control plane SHALL apply the type's action in the same transaction as the decision. If applying fails, the decision SHALL NOT be stored and the human SHALL see the error. The agent SHALL never apply an approved action itself.

#### Scenario: Action fails
- **WHEN** applying an approved action fails validation
- **THEN** the request stays `pending` and the UI shows the error

### Requirement: Decisions are delivered as messages
Every decision SHALL queue one message on the task, authored by the system principal with reason `request.decided`, stating the request, the outcome, the scope or answer and the comment, in the same transaction as the decision. The message's content SHALL be marked hidden: the agent receives it, and the request card shows the decision to humans.

#### Scenario: Answer reaches a waiting task
- **WHEN** a question of task `DF-7` (status `blocked`) is answered
- **THEN** a `request.decided` message is queued and `DF-7` becomes `queued`

#### Scenario: Answer reaches a live run
- **WHEN** a question is answered while the run of `DF-7` is awaiting input
- **THEN** the runtime receives the message as a prompt and starts the next turn

#### Scenario: Answer not shown twice
- **WHEN** a board member opens `DF-7` after its question was answered and the answer was delivered
- **THEN** the timeline shows the question card with the answer, and no separate message with the answer

### Requirement: Requests closed with their task
When a task completes, its pending requests SHALL be closed with reason `task-completed` and SHALL no longer appear as pending.

#### Scenario: Completed with an open question
- **WHEN** the agent calls `complete_task` while a question of the task is pending
- **THEN** the question is `closed` with reason `task-completed`

### Requirement: Requests and decisions are audited
Raising, deciding and closing a request SHALL each write an audit entry with the actor (agent run, human or system) and the request id.

#### Scenario: Approval audited
- **WHEN** a board member approves a request
- **THEN** an audit entry `request.approve` with the user, roles and request id exists

### Requirement: Change notifications for requests
Writes that change a request SHALL publish a `request_changed` notification with task and request id after commit.

#### Scenario: Inbox on another replica
- **WHEN** a request is raised through replica A
- **THEN** replica B receives `request_changed` naming the task and request

### Requirement: Requests closed on cancellation
When a task is cancelled, its pending requests SHALL be closed with reason `task-cancelled`.

#### Scenario: Cancelled with a pending approval
- **WHEN** a task with a pending `push-check-exception` request is cancelled
- **THEN** the request is `closed` with reason `task-cancelled` and no longer appears in the inbox

### Requirement: Approval type create-issue
The closed list of approval types SHALL include `create-issue`, with payload title (1 to 256 characters, one line), Markdown body (at most 65,535 characters) and labels (at most 20, each at most 50 characters). A payload that does not fit SHALL be rejected and nothing stored.

#### Scenario: Oversized title
- **WHEN** a `create-issue` request has a title of 300 characters
- **THEN** it is rejected and nothing is stored

#### Scenario: Identical requests merge
- **WHEN** the same title, body and labels are raised twice while the first request is pending
- **THEN** one pending request exists

### Requirement: create-issue payload is fixed
The stored payload of a `create-issue` request SHALL NOT change after the request is raised. The type SHALL offer one scope, this request only.

#### Scenario: Arguments fixed
- **WHEN** the agent calls `create_issue` again with a different body after the first request was approved
- **THEN** the approved issue has the first body and the new call raises a separate request

### Requirement: create-issue card shows the exact text
The request card SHALL show the exact title and labels as text and the full body rendered as Markdown under the rules of agent messages. The decision message SHALL name the title.

#### Scenario: Exact text shown
- **WHEN** a `create-issue` request is pending
- **THEN** its card shows the full title, all labels and the full body as stored

#### Scenario: Raw HTML in the body
- **WHEN** the body contains an HTML tag
- **THEN** the card shows it as text

### Requirement: create-issue body has a Markdown tab
The card SHALL show the body in two tabs: Preview (rendered, the default) and Markdown (the stored text, escaped, in a code block). Both SHALL come from the stored payload, and the decision actions SHALL be outside the tabs.

#### Scenario: Hidden link target
- **WHEN** the body contains `[docs](https://evil.example/x)`
- **THEN** the Markdown tab shows that exact text and the Preview tab shows the link text only

#### Scenario: Default tab
- **WHEN** the card is shown
- **THEN** the Preview tab is selected and the Markdown tab is one click away

### Requirement: Approving create-issue creates an issue
On approval of a `create-issue` request the control plane SHALL queue, in the decision's transaction, a job that creates an issue in the task's project repository from the stored payload, once per approval. It SHALL NOT call the forge inside the decision's transaction. A denial SHALL create no issue. A `transport` failure or crash after the forge created the issue but before its number was recorded MAY lead to a duplicate on retry; this is accepted.

#### Scenario: Approved
- **WHEN** a board member approves a `create-issue` request of `DF-12-CHAT`
- **THEN** one issue with the stored title, body and labels is created on the project's repository

#### Scenario: Denied
- **WHEN** a board member denies the request
- **THEN** no issue is created and the task receives the usual decision message

### Requirement: Outcome of the issue job
When the job ends, it SHALL queue one hidden message on the task with the new issue's number and URL, or with the forge's error. A rate-limited or transport failure SHALL be retried; `invalid-input` and `unauthorized` SHALL NOT. After the number is recorded, the job SHALL NOT run again.

#### Scenario: Number reported
- **WHEN** the issue was created
- **THEN** `DF-12-CHAT` receives a hidden message with its number and URL

#### Scenario: Unknown label
- **WHEN** an approved request names a label the repository does not have
- **THEN** no issue is created, the task receives a message with the forge's error, and the approved request card shows that error

#### Scenario: Forge down at approval
- **WHEN** a board member approves while the forge is unreachable
- **THEN** the decision is stored and the job retries for up to an hour

#### Scenario: Forge down for over an hour
- **WHEN** the forge stays unreachable for more than an hour after the approval
- **THEN** the job gives up, the request records the last forge error, the task receives a hidden message with that error, and the approved request card shows it
