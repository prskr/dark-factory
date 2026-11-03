# Spec Delta

## ADDED Requirements

### Requirement: Artifact limit values
The chart SHALL expose `artifacts.maxTextBytes` (default 1048576), `artifacts.maxImageBytes` (default 3932160) and `artifacts.maxPerTask` (default 50) and render them under `artifacts` of the instance configuration. The values schema SHALL reject byte limits below 1 or above 8388608 (8 MiB, the most the runtime sends) and a per-task count below 1.

#### Scenario: Defaults
- **WHEN** a release is rendered without artifact values
- **THEN** the instance configuration has `maxTextBytes: 1048576`, `maxImageBytes: 3932160` and `maxPerTask: 50` under `artifacts`

#### Scenario: Image limit above the runtime cap
- **WHEN** `artifacts.maxImageBytes` is 10485760
- **THEN** rendering fails with a schema error
