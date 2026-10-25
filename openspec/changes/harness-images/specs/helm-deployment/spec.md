# Spec Delta

## ADDED Requirements

### Requirement: Harness image values
The chart SHALL offer `image.harness.claudeCode.repository`, `.tag` and `.digest` (digest wins over tag, an empty tag falls back to the chart `appVersion`) and render the resulting reference as `harness.claudeCode.image` of the instance configuration.

#### Scenario: Tag
- **WHEN** the repository is `registry.example.org/claude-harness` and the tag `0.95.0`
- **THEN** the instance configuration has `image: "registry.example.org/claude-harness:0.95.0"`

#### Scenario: Digest
- **WHEN** a digest `sha256:…` is set
- **THEN** the reference is `registry.example.org/claude-harness@sha256:…`

### Requirement: Image volumes and Kubernetes version
The chart SHALL declare `kubeVersion: ">=1.36.0-0"` and SHALL NOT offer a runtime delivery setting; the runtime and harness images are always delivered as image volumes.

#### Scenario: Older cluster
- **WHEN** the chart is installed on Kubernetes 1.35
- **THEN** Helm refuses the installation with the unsatisfied `kubeVersion`

#### Scenario: No delivery setting
- **WHEN** the chart is rendered
- **THEN** the instance configuration has no `runtimeDelivery` key and `values.yaml` has no `sandboxNamespace.runtimeDelivery`
