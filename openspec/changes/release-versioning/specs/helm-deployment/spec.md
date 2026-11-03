# Spec Delta

## ADDED Requirements

### Requirement: First-party image tags follow the chart version
The control plane, runtime and test project image values SHALL default to an empty tag, so that each renders as `<repository>:<chart appVersion>`. A set digest SHALL win over the tag, and a set tag SHALL win over `appVersion`.

#### Scenario: Default values
- **WHEN** chart `2026.11.0` is rendered with its default values
- **THEN** the control plane image is `ghcr.io/prskr/dark-factory/control-plane:2026.11.0` and the runtime image `ghcr.io/prskr/dark-factory/runtime:2026.11.0`

#### Scenario: Pinned tag
- **WHEN** `image.controlPlane.tag` is `2026.10.3`
- **THEN** the control plane image is `ghcr.io/prskr/dark-factory/control-plane:2026.10.3`
