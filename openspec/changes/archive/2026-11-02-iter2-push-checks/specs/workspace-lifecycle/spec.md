# Spec Delta

## ADDED Requirements

### Requirement: Push rejected by a check
When the Git proxy rejects a push with a `df:` reason, the runtime SHALL send a `PushReport` with `REJECTED_SECRET_DETECTED` for `df:secret-scan`; `REJECTED_BY_CHECK` for `df:identity`, `df:protected-path`, `df:blob-too-large`, `df:merge-commit` and `df:push-too-large`; `REJECTED_NON_FAST_FORWARD` for `df:non-fast-forward`; and `FAILED` for `df:audit-unavailable` and any other reason. The detail SHALL be the reason text, and the rejected commits SHALL count as not pushed.

#### Scenario: Rejected after a turn
- **WHEN** the push after a turn is rejected with `df:protected-path`
- **THEN** the runtime sends `PushReport { status: REJECTED_BY_CHECK }` with the reason as detail

#### Scenario: Retry after the next turn
- **WHEN** a push was rejected and the next turn produces no new commits
- **THEN** the runtime pushes the task branch again after that turn

#### Scenario: Audit unavailable
- **WHEN** the push is rejected with `df:audit-unavailable`
- **THEN** the runtime sends `PushReport { status: FAILED }` with the reason as detail and pushes again after the next turn

#### Scenario: Ref rule rejection
- **WHEN** the push is rejected with `df:non-fast-forward`
- **THEN** the runtime sends `PushReport { status: REJECTED_NON_FAST_FORWARD }` and never force-pushes
