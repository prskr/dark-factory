# Spec Delta

## ADDED Requirements

### Requirement: Service version value
The `service.version` resource attribute SHALL be the release version the binary was built with, or `dev` for a binary built without a version. A `service.version` set in `OTEL_RESOURCE_ATTRIBUTES` SHALL override it, as for every resource attribute from that variable.

#### Scenario: Release build
- **WHEN** the control plane from release `2026.11.0` exports metrics
- **THEN** `target_info` carries `service_version="2026.11.0"`

#### Scenario: Dev build
- **WHEN** a control plane built with plain `cargo build` exports metrics
- **THEN** `target_info` carries `service_version="dev"`

#### Scenario: Operator override
- **WHEN** the control plane from release `2026.11.0` runs with `OTEL_RESOURCE_ATTRIBUTES=service.version=custom`
- **THEN** `target_info` carries `service_version="custom"`
