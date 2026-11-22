# Spec Delta

## ADDED Requirements

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
- **THEN** no issue is created and the task receives a message with the forge's error

#### Scenario: Forge down at approval
- **WHEN** a board member approves while the forge is unreachable
- **THEN** the decision is stored and the job retries until the forge answers
