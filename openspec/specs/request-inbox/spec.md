# request-inbox Specification

## Purpose

Lets humans see every pending question and approval request and decide it quickly, on a phone or a desktop (ADR-0038, ADR-0035).

## Requirements

### Requirement: Pending requests page
The UI SHALL list all pending requests across tasks, newest first, with task key and title, kind, type, the agent's text or reason and the time raised, and SHALL update live when requests change.

#### Scenario: New request appears
- **WHEN** an agent raises a question while a board member has the inbox open
- **THEN** the question appears without a page reload

### Requirement: Pending count in the navigation
The navigation SHALL show the number of pending requests on every page and update it live.

#### Scenario: Count after a decision
- **WHEN** one of three pending requests is decided
- **THEN** the navigation shows 2

### Requirement: Requests on the task timeline
The task detail SHALL show each request of the task as a card on the timeline at the time it was raised, with its state, decision, decider and the actions for pending requests.

#### Scenario: Decided request in history
- **WHEN** a board member opens a task whose question was answered yesterday
- **THEN** the timeline shows the question card with the answer and who gave it

### Requirement: Deciding from the UI
For a pending question the UI SHALL offer the suggested answers as one-tap buttons and a free-text answer. For an approval request it SHALL offer approve with a scope chosen from the type's scopes (preselected: the narrowest) and deny with an optional comment.

#### Scenario: One-tap answer on a phone
- **WHEN** a board member taps the suggested answer "17" at 390 px width
- **THEN** the question is answered with "17" without another confirmation step

### Requirement: Agent text is untrusted
Request text, reasons and payload values SHALL be rendered escaped, Markdown without raw HTML, and links SHALL NOT be followed automatically.

#### Scenario: Script in a reason
- **WHEN** an approval reason contains `<script>alert(1)</script>`
- **THEN** it is shown as text

### Requirement: Phone-ready inbox
The inbox and request cards SHALL be usable at 390 px width and appear in the mobile navigation.

#### Scenario: Phone navigation
- **WHEN** the UI is opened at 390 px width
- **THEN** the navigation contains the inbox with its pending count

### Requirement: Deny always
A pending `network-access` request SHALL offer "Deny always" next to approve and deny, with the same optional comment and permission as deny. It SHALL deny the request and create a deny rule for exactly the requested host in one step. Other pending requests matching that host SHALL then be denied as for any new deny rule.

#### Scenario: Deny always from the inbox
- **WHEN** a board member chooses "Deny always" on the request of `DF-7` for `http-intake.logs.us5.datadoghq.com:443`
- **THEN** the request is `denied` by the board member, deny rule `http-intake.logs.us5.datadoghq.com` exists and `DF-7` receives a `request.decided` message

#### Scenario: Exact host only
- **WHEN** a board member chooses "Deny always" on a request for `http-intake.logs.us5.datadoghq.com:443`
- **THEN** the deny rule is `http-intake.logs.us5.datadoghq.com`, not `*.datadoghq.com`

#### Scenario: Phone
- **WHEN** a board member opens a `network-access` request at 390 px width
- **THEN** "Deny always" is offered
