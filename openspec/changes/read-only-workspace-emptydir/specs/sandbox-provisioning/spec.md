# Spec Delta

## MODIFIED Requirements

### Requirement: Task volume
The control plane SHALL create one ReadWriteOnce PVC per task with a writable workspace named `task-<key lower case>` in the sandbox namespace, using the configured StorageClass (the cluster's default StorageClass when none is configured) and size, before the task's first run and before any run that follows a volume deletion, and SHALL keep it after runs end. It SHALL NOT create a PVC for a task with a read-only workspace.

#### Scenario: First run of DF-1
- **WHEN** run 1 of `DF-1` is provisioned
- **THEN** PVC `task-df-1` exists and is mounted at `/data`

#### Scenario: Second run reuses the volume
- **WHEN** run 2 of `DF-1` is provisioned
- **THEN** no new PVC is created and the same PVC is mounted

#### Scenario: Recreated on the next run
- **WHEN** the volume of `DF-1` was deleted and a message is queued on `DF-1`
- **THEN** a new PVC `task-df-1` is created before the run and the runtime clones afresh

#### Scenario: No PVC for a read-only workspace
- **WHEN** a run of `DF-12-CHAT` is provisioned
- **THEN** no PVC `task-df-12-chat` is created

### Requirement: Sandbox per run
The control plane SHALL create one bare Pod per run (`restartPolicy: Never`) in the sandbox namespace with: the project image; entrypoint `/opt/factory/factory-agent run`; the runtime image read-only at `/opt/factory`, the run's harness image at `/opt/harness/<harness>` unless the project sets an adapter command; the data volume at `/data`; a memory volume at `/run/factory`; a cert-manager csi volume with the per-pod URI SAN; labels for instance, run, task, project and persona.

#### Scenario: Sandbox spec
- **WHEN** run 1 of `DF-1` is provisioned and project `DF` has no adapter command
- **THEN** pod `run-df-1-1` has volumes `/opt/factory` (image, read-only), `/opt/harness/claude-code` (image, read-only), `/data` (PVC `task-df-1`), `/run/factory` (memory) and the csi certificate, and label `dark-factory.io/run-id` with the run id

#### Scenario: Project with its own adapter
- **WHEN** a run of a project that sets an adapter command is provisioned
- **THEN** its pod has no `/opt/harness/claude-code` volume

#### Scenario: Plain pod without restarts
- **WHEN** run 1 of `DF-1` is provisioned
- **THEN** pod `run-df-1-1` has `restartPolicy: Never` and no owner reference

#### Scenario: Provisioning again is idempotent
- **WHEN** run 1 of `DF-1` is provisioned while pod `run-df-1-1` already exists
- **THEN** the call succeeds and the existing pod is kept

#### Scenario: No agent-sandbox needed
- **WHEN** the cluster has no `agents.x-k8s.io` resources installed
- **THEN** runs are provisioned and torn down normally

## ADDED Requirements

### Requirement: Data volume by workspace mode
The data volume at `/data` SHALL be the task PVC for a task with a writable workspace and the read-only data volume for a task with a read-only workspace. The choice SHALL follow the task's read-only workspace property, not its key or kind.

#### Scenario: Writable workspace
- **WHEN** run 1 of `DF-1` is provisioned
- **THEN** its pod mounts PVC `task-df-1` at `/data`

#### Scenario: Read-only workspace
- **WHEN** a run of `DF-12-CHAT` is provisioned
- **THEN** its pod mounts an `emptyDir` at `/data` and references no PVC

### Requirement: Read-only data volume
The pod of a run whose task has a read-only workspace SHALL mount at `/data` an `emptyDir` on node storage whose `sizeLimit` is `sandbox.readOnlyDataSizeLimit` (optional, a Kubernetes quantity, default `10Gi`). The control plane SHALL refuse to start when the value is not a valid quantity, with an error naming the option. The volume SHALL end with the pod.

#### Scenario: Default size limit
- **WHEN** a run of `DF-12-CHAT` is provisioned and `sandbox.readOnlyDataSizeLimit` is unset
- **THEN** the pod's `/data` volume is an `emptyDir` with `sizeLimit: 10Gi` and no `medium`

#### Scenario: Configured size limit
- **WHEN** `sandbox.readOnlyDataSizeLimit` is `4Gi`
- **THEN** pods of read-only workspaces get an `emptyDir` with `sizeLimit: 4Gi` and pods of writable workspaces are unchanged

#### Scenario: Volume follows the property, not the key
- **WHEN** a run of a task whose read-only workspace property is true is provisioned, whatever its key
- **THEN** its pod mounts an `emptyDir` at `/data` and no PVC is created

#### Scenario: Invalid size limit
- **WHEN** `sandbox.readOnlyDataSizeLimit` is `lots`
- **THEN** the control plane exits at startup with an error naming `sandbox.readOnlyDataSizeLimit`

#### Scenario: Next run starts empty
- **WHEN** a run of `DF-12-CHAT` ended and its pod was deleted, and the next run starts
- **THEN** `/data` of the new pod is empty before the runtime prepares it
