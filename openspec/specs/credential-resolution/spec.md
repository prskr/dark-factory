# credential-resolution Specification

## Purpose

Resolves credential references (Secret name + key) to their values at the moment of use, so secrets live only in Kubernetes Secrets in the instance namespace (ADR-0020).

## Requirements

### Requirement: Resolve references at use time
The control plane SHALL read the referenced key of the referenced Secret in the instance namespace each time a credential is needed, and SHALL keep values in memory only for the duration of use.

#### Scenario: Rotated Secret
- **WHEN** a Secret's value changes between two runs
- **THEN** the second run receives the new value

#### Scenario: Missing key
- **WHEN** the Secret exists but lacks the referenced key
- **THEN** resolution fails with an error naming Secret and key, without any value

### Requirement: Secret values never logged
Resolved values SHALL be wrapped so that debug and display output redact them.

#### Scenario: Debug logging
- **WHEN** a resolved credential is logged at debug level
- **THEN** the log shows a redaction marker instead of the value
