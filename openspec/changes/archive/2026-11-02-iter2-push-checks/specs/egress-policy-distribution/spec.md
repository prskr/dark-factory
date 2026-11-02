# Spec Delta

## ADDED Requirements

### Requirement: Push policies and exceptions in the snapshot
The policy snapshot SHALL carry each active run's project push policy and the run's task exceptions, and SHALL be updated within one second after a policy change or an approved exception.

#### Scenario: Exception reaches the gateway
- **WHEN** a `push-check-exception` request of `DF-7` is approved through replica A
- **THEN** a gateway connected to replica B has the exception for `DF-7` within one second
