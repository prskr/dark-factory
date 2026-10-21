# Spec Delta

## ADDED Requirements

### Requirement: Push rejected by a check
When the Git proxy rejects a push with a `df:` reason, the runtime SHALL send a `PushReport` with `REJECTED_SECRET_DETECTED` for `df:secret-scan`, `REJECTED_BY_CHECK` for other `df:` reasons, and the reason text as detail. The rejected commits SHALL count as not pushed.

#### Scenario: Rejected after a turn
- **WHEN** the push after a turn is rejected with `df:protected-path`
- **THEN** the runtime sends `PushReport { status: REJECTED_BY_CHECK }` with the reason as detail

#### Scenario: Retry after the next turn
- **WHEN** a push was rejected and the next turn produces no new commits
- **THEN** the runtime pushes the task branch again after that turn
