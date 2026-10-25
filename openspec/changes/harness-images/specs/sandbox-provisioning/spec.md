# Spec Delta

## MODIFIED Requirements

### Requirement: Sandbox per run
The control plane SHALL create one `Sandbox` per run in the sandbox namespace with: the project image; entrypoint `/opt/factory/factory-agent run`; the runtime image as a read-only image volume at `/opt/factory`; the harness image of the run's harness as a read-only image volume at `/opt/harness/<harness>` unless the project sets an adapter command; the task PVC at `/data`; a memory-backed volume at `/run/factory`; a cert-manager csi-driver volume with the per-pod URI SAN; labels for instance, run, task, project and persona.

#### Scenario: Sandbox spec
- **WHEN** run 1 of `DF-1` is provisioned and project `DF` has no adapter command
- **THEN** its pod has volumes `/opt/factory` (image, read-only), `/opt/harness/claude-code` (image, read-only), `/data` (PVC `task-df-1`), `/run/factory` (memory) and the csi certificate, and label `dark-factory.io/run-id` with the run id

#### Scenario: Project with its own adapter
- **WHEN** a run of a project that sets an adapter command is provisioned
- **THEN** its pod has no `/opt/harness/claude-code` volume

## ADDED Requirements

### Requirement: Harness image from the instance configuration
The harness image mounted for a run SHALL be the image configured for the run's harness (`harness.claudeCode.image` for `claude-code`). A run whose harness has no configured image and whose project sets no adapter command SHALL fail provisioning with a message naming the harness.

#### Scenario: Configured image
- **WHEN** `harness.claudeCode.image` is `registry.example.org/claude-harness:0.95.0`
- **THEN** the pod's `/opt/harness/claude-code` volume references that image

#### Scenario: No image configured
- **WHEN** the harness of a run has no configured image and the project sets no adapter command
- **THEN** provisioning fails and the run ends `provisioning_failed` with a message naming the harness

### Requirement: Image volumes only
The runtime and the harness SHALL be delivered only as Kubernetes image volumes; no init-container delivery SHALL exist. The control plane's Kubernetes version SHALL be at least 1.36.

#### Scenario: Pod without init containers
- **WHEN** a sandbox pod is rendered
- **THEN** it has no init container for delivering the runtime or the harness
