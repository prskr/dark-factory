# Spec Delta

## ADDED Requirements

### Requirement: Transient Git failures are retried
Clone, fetch and push through the Git proxy SHALL be retried up to 3 times with delays of 1 s, 5 s and 15 s when they fail with a connection error or HTTP 503 (gateway pod without a snapshot, forge unreachable). Any other failure SHALL NOT be retried: rejections reported by the proxy (`remote rejected`, 403) and 502 (the forge rejected the proxy's credential). Before retrying a push the runtime SHALL re-read the remote ref; if it already equals the pushed SHA (the previous attempt's response was lost after the ref update), the push SHALL count as successful and be reported as such.

#### Scenario: Gateway rollout during clone
- **WHEN** the first clone attempt gets a 503 from a gateway pod that has no snapshot yet
- **THEN** the runtime retries and the clone succeeds against a ready pod without the run failing

#### Scenario: Forge unavailable during push
- **WHEN** a push gets 503 because the forge is unreachable and the forge is back 10 seconds later
- **THEN** the second or third attempt succeeds and one `PushReport` with status `OK` is sent

#### Scenario: Credential rejected upstream
- **WHEN** a push gets 502 because the forge rejected the proxy's credential
- **THEN** the runtime doesn't retry and sends one `PushReport` with status `FAILED`

#### Scenario: Rejected push is not retried
- **WHEN** a push is rejected with `df:ref-not-allowed`
- **THEN** the runtime reports it once and doesn't push again in that turn

#### Scenario: Response lost after the ref update
- **WHEN** the proxy applied a task-branch push upstream but the connection dropped before `report-status` reached the runtime
- **THEN** the retry finds the remote ref at the pushed SHA and the runtime sends one `PushReport` with status `OK`
