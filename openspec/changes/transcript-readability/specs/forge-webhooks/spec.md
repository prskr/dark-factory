# Spec Delta

## MODIFIED Requirements

### Requirement: Wake-up message per event
Every accepted review, CI failure, mention and closed-without-merge event SHALL queue one message from the system principal (reason `webhook`) on each task it routes to, in the same transaction that processes the event. The message SHALL carry the event's structured fields (kind, actor, PR or issue number, review verdict or check name, URL) for display. Like other queued messages, messages arriving during a turn SHALL be held back and forwarded after it, so a burst of events wakes the task once and never starts a second run.

#### Scenario: Review and CI failure together
- **WHEN** a review is submitted on the PR of `DF-1` in status `in_review` and 10 s later CI fails on its branch
- **THEN** `DF-1` has two new messages, becomes `queued` once, and its PR is marked draft once

#### Scenario: Structured event stored
- **WHEN** `alice` submits a review requesting changes on PR #12 of `DF-1`
- **THEN** the queued message carries kind review, actor `alice`, verdict changes requested, PR #12 and the PR URL

### Requirement: Message content
The wake-up message SHALL name, per event, its kind, the actor's login, the review verdict or the CI check name, and the PR or issue number, and SHALL point to `read_pull_request_feedback` for reviews and comments. It SHALL NOT contain review, comment or CI log text from the payload. Its text SHALL be a hidden content block: delivered to the agent, not shown in the transcript.

#### Scenario: Review message
- **WHEN** `alice` submits a review requesting changes on PR #12 of `DF-1`
- **THEN** the message says that `alice` requested changes on PR #12 and to read the feedback with `read_pull_request_feedback`, and contains no part of the review body

#### Scenario: Text hidden in the transcript
- **WHEN** the message is delivered and `web.showHiddenBlocks` is `false`
- **THEN** the agent receives the text and the transcript shows no message bubble for it
