# Spec Delta

## MODIFIED Requirements

### Requirement: Usage per run and model
The usage of a run SHALL be the sum of its known records per model, plus the number of requests with unknown usage. For a run whose records were purged with its deleted task, the usage SHALL be the per-model totals stored before the records were removed, with the same values as before the purge.

#### Scenario: Two models in one run
- **WHEN** run R has two records for `claude-sonnet-5-5` with 100 and 50 input tokens and one for `claude-haiku-5-5` with 10
- **THEN** R's usage shows 150 input tokens for `claude-sonnet-5-5` and 10 for `claude-haiku-5-5`

#### Scenario: Unknown usage
- **WHEN** run R has one record with unknown usage
- **THEN** R's usage shows one request with unknown usage and no tokens for it

#### Scenario: Usage of a deleted task
- **WHEN** run R of `DF-1` used 150 input tokens of `claude-sonnet-5-5` and `DF-1` is deleted and purged
- **THEN** R's usage still shows 150 input tokens for `claude-sonnet-5-5`, and the sums per task, persona and project are unchanged
