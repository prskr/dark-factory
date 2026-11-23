# Spec Delta

## ADDED Requirements

### Requirement: Read-only child completes once
When a run of a child with a read-only workspace ends `completed`, its parent SHALL receive one `task.child_ended` message saying it completed, with its completion summary and without a PR link. No second message for Done SHALL follow, because the child is already Done. A message from the parent (for example "please review again") SHALL reopen it as any Done child, and its next completion SHALL wake the parent again.

#### Scenario: Review done
- **WHEN** read-only child `DF-21` of `DF-19` ends a run `completed` with summary "Two findings, see PR #12"
- **THEN** `DF-19` receives exactly one `task.child_ended` message for it, containing the summary

#### Scenario: Review again
- **WHEN** the agent of `DF-19` sends "please review again" to Done `DF-21`, and the new run of `DF-21` ends `completed`
- **THEN** `DF-21` is `queued` after the message, and `DF-19` receives a second `task.child_ended` message after the new run
