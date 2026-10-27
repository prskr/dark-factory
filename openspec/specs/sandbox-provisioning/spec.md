# sandbox-provisioning Specification

## Purpose

Defines the Kubernetes resources that host a run — one cold-started agent-sandbox `Sandbox` per run and one persistent volume per task — and their lifecycle (`design/agent-run.md` §5, ADR-0022, ADR-0030).

## Requirements

### Requirement: Task volume
The control plane SHALL create one ReadWriteOnce PVC per task named `task-<key lower case>` in the sandbox namespace, using the configured StorageClass (the cluster's default StorageClass when none is configured) and size, before the task's first run, and SHALL keep it after runs end.

#### Scenario: First run of DF-1
- **WHEN** run 1 of `DF-1` is provisioned
- **THEN** PVC `task-df-1` exists and is mounted at `/data`

#### Scenario: Second run reuses the volume
- **WHEN** run 2 of `DF-1` is provisioned
- **THEN** no new PVC is created and the same PVC is mounted

### Requirement: Sandbox per run
The control plane SHALL create one `Sandbox` per run in the sandbox namespace with: the project image; entrypoint `/opt/factory/factory-agent run`; the runtime image as a read-only image volume at `/opt/factory`; the harness image of the run's harness as a read-only image volume at `/opt/harness/<harness>` unless the project sets an adapter command; the task PVC at `/data`; a memory-backed volume at `/run/factory`; a cert-manager csi-driver volume with the per-pod URI SAN; labels for instance, run, task, project and persona.

#### Scenario: Sandbox spec
- **WHEN** run 1 of `DF-1` is provisioned and project `DF` has no adapter command
- **THEN** its pod has volumes `/opt/factory` (image, read-only), `/opt/harness/claude-code` (image, read-only), `/data` (PVC `task-df-1`), `/run/factory` (memory) and the csi certificate, and label `dark-factory.io/run-id` with the run id

#### Scenario: Project with its own adapter
- **WHEN** a run of a project that sets an adapter command is provisioned
- **THEN** its pod has no `/opt/harness/claude-code` volume

### Requirement: Sandbox runtime class
Sandbox pods SHALL use the configured runtime class. When none is configured (unset or empty), they SHALL have no `runtimeClassName` and run with the cluster's default runtime.

#### Scenario: Runtime class configured
- **WHEN** run 1 of `DF-1` is provisioned with runtime class `kata` configured
- **THEN** its pod has runtime class `kata`

#### Scenario: No runtime class configured
- **WHEN** run 1 of `DF-1` is provisioned and no runtime class is configured (unset or empty)
- **THEN** its pod has no `runtimeClassName` and runs with the cluster's default runtime

### Requirement: Restricted pod security
Sandbox pods SHALL run non-root with no privilege escalation, all capabilities dropped, the RuntimeDefault seccomp profile and `automountServiceAccountToken: false`, using a ServiceAccount without permissions.

#### Scenario: No Kubernetes API token
- **WHEN** the agent lists `/var/run/secrets/kubernetes.io/serviceaccount` in the sandbox
- **THEN** the directory doesn't exist

### Requirement: Teardown keeps the volume
Tearing down a run SHALL delete its Sandbox (and with it the pod and network policy) and SHALL keep the task PVC. Teardown of an already deleted Sandbox SHALL succeed.

#### Scenario: Run ended
- **WHEN** the run of `DF-1` is torn down
- **THEN** the Sandbox and its pod are gone and PVC `task-df-1` still exists

### Requirement: Sandbox events
The provider SHALL report when a run's pod is running with its pod IP, and when a run's pod or Sandbox is gone or failed, including the reason.

#### Scenario: Pod started
- **WHEN** the run's pod becomes Running with IP 10.0.3.7
- **THEN** a `Started` event with the run id and 10.0.3.7 is emitted

#### Scenario: Image pull fails
- **WHEN** the project image can't be pulled
- **THEN** a `Gone` event with a reason naming the image pull failure is emitted

### Requirement: Startup verification
At startup the control plane SHALL verify that the configured sandbox namespace exists and that it holds the permissions it needs there, and SHALL refuse to start with a message naming each missing item. It SHALL never create the namespace.

#### Scenario: Missing permission
- **WHEN** the control plane may not create `persistentvolumeclaims` in the sandbox namespace
- **THEN** it exits non-zero with a message naming that permission

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
