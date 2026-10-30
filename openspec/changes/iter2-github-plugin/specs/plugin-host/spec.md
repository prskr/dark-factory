# Spec Delta

## MODIFIED Requirements

### Requirement: Retries per contract
The host SHALL retry calls failing with `rate-limited` or `transport`, at most 3 times, and SHALL NOT retry other errors. A `rate-limited` call SHALL be retried after its retry-after (1 s without one); when the retry-after exceeds the cap of 30 s, the host SHALL return the error without retrying.

#### Scenario: Rate-limited comment
- **WHEN** `add-comment` returns `rate-limited` with retry-after 2 s
- **THEN** the host waits 2 s and retries

#### Scenario: Retry-after beyond the cap
- **WHEN** `create` returns `rate-limited` with retry-after 600 s
- **THEN** the host returns the error at once and sends no further request
