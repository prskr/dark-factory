# Spec Delta

## ADDED Requirements

### Requirement: Final report delivery
After finalize the runtime SHALL keep reconnecting with the usual backoff until every outbox message including the `RunReport` is acknowledged, for up to 10 minutes, and only then exit. If the window passes, it SHALL exit with the unacknowledged messages left in the outbox. The window does not extend the run's wall-clock budget: the control plane's timeout enforcement applies unchanged.

#### Scenario: Control plane restarts during finalize
- **WHEN** the control plane is unreachable for 60 seconds while the runtime finalizes, within the run's remaining wall-clock budget and grace
- **THEN** the runtime keeps the pod alive, reconnects, delivers the `RunReport` and exits afterwards

#### Scenario: Flush window exceeded
- **WHEN** the control plane stays unreachable for more than 10 minutes after finalize
- **THEN** the runtime exits; the control plane later ends the run `sandbox_lost` and resumes the task
