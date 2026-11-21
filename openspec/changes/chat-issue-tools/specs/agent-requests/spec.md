# Spec Delta

## ADDED Requirements

### Requirement: Approval type create-issue
The closed list of approval types SHALL include `create-issue`, with payload title (1 to 256 characters, one line), Markdown body (at most 65,535 characters) and labels (at most 20, each at most 50 characters). A payload that does not fit SHALL be rejected and nothing stored. The stored payload SHALL NOT change after the request is raised. Its fingerprint SHALL cover title, body and labels, so identical pending requests merge. The request card and the decision message SHALL show the exact title, labels and body. The type SHALL offer one scope, this request only.

#### Scenario: Exact text shown
- **WHEN** a `create-issue` request is pending
- **THEN** its card shows the full title, all labels and the full body as stored

#### Scenario: Oversized title
- **WHEN** a `create-issue` request has a title of 300 characters
- **THEN** it is rejected and nothing is stored

### Requirement: Approving create-issue creates one issue
On approval of a `create-issue` request the control plane SHALL queue, in the decision's transaction, a job that creates an issue in the task's project repository from the stored payload. It SHALL NOT call the forge inside the decision's transaction. When the job ends, it SHALL queue one hidden message on the task with the new issue's number and URL, or with the forge's error. A rate-limited or transport failure SHALL be retried; `invalid-input` and `unauthorized` SHALL NOT. After the number is recorded, the job SHALL NOT run again. A denial SHALL create no issue.

#### Scenario: Approved
- **WHEN** a board member approves a `create-issue` request of `DF-12-CHAT`
- **THEN** one issue with the stored title, body and labels is created on the project's repository and `DF-12-CHAT` receives a hidden message with its number and URL

#### Scenario: Denied
- **WHEN** a board member denies the request
- **THEN** no issue is created and the task receives the usual decision message

#### Scenario: Unknown label
- **WHEN** an approved request names a label the repository does not have
- **THEN** no issue is created and the task receives a message with the forge's error

#### Scenario: Forge down at approval
- **WHEN** a board member approves while the forge is unreachable
- **THEN** the decision is stored and the job retries until the forge answers

#### Scenario: Arguments fixed
- **WHEN** the agent calls `create_issue` again with a different body after the first request was approved
- **THEN** the approved issue has the first body and the new call raises a separate request
