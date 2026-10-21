# Spec Delta

## ADDED Requirements

### Requirement: Transient Git failures are retried
Clone, fetch and push through the Git proxy SHALL be retried up to 3 times with delays of 1 s, 5 s and 15 s when they fail with a connection error or an HTTP 5xx status. Rejections reported by the proxy (`remote rejected`, 403) SHALL NOT be retried. Before retrying a push the runtime SHALL re-read the remote ref; if it already equals the pushed SHA (the previous attempt's response was lost after the ref update), the push SHALL count as successful and be reported as such.

#### Scenario: Gateway rollout during clone
- **WHEN** the first clone attempt gets a 503 from a gateway pod that has no snapshot yet
- **THEN** the runtime retries and the clone succeeds against a ready pod without the run failing

#### Scenario: Rejected push is not retried
- **WHEN** a push is rejected with `df:ref-not-allowed`
- **THEN** the runtime reports it once and doesn't push again in that turn

#### Scenario: Response lost after the ref update
- **WHEN** the proxy applied a task-branch push upstream but the connection dropped before `report-status` reached the runtime
- **THEN** the retry finds the remote ref at the pushed SHA and the runtime sends one `PushReport` with status `OK`
