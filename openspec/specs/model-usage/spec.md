# model-usage Specification

## Purpose
Token usage of model requests as measured by the gateway: one record per request, stored by the control plane and aggregated per run and model. It is the only source of usage data (ADR-0050).

## Requirements

### Requirement: Usage records
The control plane SHALL store each usage record reported by a gateway with request ID, run, time, model, wire API, upstream status, input, output, cache-read and cache-write tokens, and whether usage is known. A record with a request ID that is already stored SHALL be ignored.

#### Scenario: Retried report
- **WHEN** a gateway reports the same batch twice after a timeout
- **THEN** each request is stored once

#### Scenario: Run that already ended
- **WHEN** a record arrives for a run that ended a minute ago
- **THEN** it is stored for that run

### Requirement: Usage per run and model
The usage of a run SHALL be the sum of its known records per model, plus the number of requests with unknown usage.

#### Scenario: Two models in one run
- **WHEN** run R has two records for `claude-sonnet-5-5` with 100 and 50 input tokens and one for `claude-haiku-5-5` with 10
- **THEN** R's usage shows 150 input tokens for `claude-sonnet-5-5` and 10 for `claude-haiku-5-5`

#### Scenario: Unknown usage
- **WHEN** run R has one record with unknown usage
- **THEN** R's usage shows one request with unknown usage and no tokens for it

### Requirement: No usage from the runtime
The control plane SHALL take usage only from gateway records; turn ends carry no usage.

#### Scenario: Harness reports usage over ACP
- **WHEN** the harness reports usage for a turn
- **THEN** the runtime does not forward it and the run's usage only contains gateway records
