# Spec Delta

## Purpose

One model for everything an agent needs from a human: questions with free-text answers and typed approval requests whose action the control plane applies, with one audit trail (ADR-0038).

## ADDED Requirements

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
Every decision SHALL queue one message on the task, authored by the system principal with reason `request.decided`, stating the request, the outcome, the scope or answer and the comment, in the same transaction as the decision.

#### Scenario: Answer reaches a waiting task
- **WHEN** a question of task `DF-7` (status `blocked`) is answered
- **THEN** a `request.decided` message is queued and `DF-7` becomes `queued`

#### Scenario: Answer reaches a live run
- **WHEN** a question is answered while the run of `DF-7` is awaiting input
- **THEN** the runtime receives the message as a `Prompt` and starts the next turn

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
