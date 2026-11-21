# Spec Delta

## MODIFIED Requirements

### Requirement: Task volume
The control plane SHALL create one ReadWriteOnce PVC per task of kind `task` named `task-<key lower case>` in the sandbox namespace, using the configured StorageClass (the cluster's default StorageClass when none is configured) and size, before the task's first run and before any run that follows a volume deletion, and SHALL keep it after runs end. It SHALL NOT create a PVC for a chat.

#### Scenario: First run of DF-1
- **WHEN** run 1 of `DF-1` is provisioned
- **THEN** PVC `task-df-1` exists and is mounted at `/data`

#### Scenario: Second run reuses the volume
- **WHEN** run 2 of `DF-1` is provisioned
- **THEN** no new PVC is created and the same PVC is mounted

#### Scenario: Recreated on the next run
- **WHEN** the volume of `DF-1` was deleted and a message is queued on `DF-1`
- **THEN** a new PVC `task-df-1` is created before the run and the runtime clones afresh

#### Scenario: No PVC for a chat
- **WHEN** a run of `DF-12-CHAT` is provisioned
- **THEN** no PVC `task-df-12-chat` is created

### Requirement: Sandbox per run
The control plane SHALL create one bare Pod per run (`restartPolicy: Never`) in the sandbox namespace with: the project image; entrypoint `/opt/factory/factory-agent run`; the runtime image read-only at `/opt/factory`, the run's harness image at `/opt/harness/<harness>` unless the project sets an adapter command; at `/data` the task PVC for a task of kind `task`, or the chat data volume for a chat; a memory volume at `/run/factory`; a cert-manager csi volume with the per-pod URI SAN; labels for instance, run, task, project and persona.

#### Scenario: Sandbox spec
- **WHEN** run 1 of `DF-1` is provisioned and project `DF` has no adapter command
- **THEN** pod `run-df-1-1` has volumes `/opt/factory` (image, read-only), `/opt/harness/claude-code` (image, read-only), `/data` (PVC `task-df-1`), `/run/factory` (memory) and the csi certificate, and label `dark-factory.io/run-id` with the run id

#### Scenario: Chat sandbox spec
- **WHEN** a run of `DF-12-CHAT` is provisioned
- **THEN** its pod mounts an `emptyDir` at `/data` and references no PVC

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

### Requirement: Chat data volume
A chat run's pod SHALL mount at `/data` an `emptyDir` on node storage whose `sizeLimit` is `sandbox.chatDataSizeLimit` (optional, a Kubernetes quantity, default `10Gi`). The control plane SHALL refuse to start when the value is not a valid quantity, with an error naming the option. The volume SHALL end with the pod.

#### Scenario: Default size limit
- **WHEN** a run of `DF-12-CHAT` is provisioned and `sandbox.chatDataSizeLimit` is unset
- **THEN** the pod's `/data` volume is an `emptyDir` with `sizeLimit: 10Gi` and no `medium`

#### Scenario: Configured size limit
- **WHEN** `sandbox.chatDataSizeLimit` is `4Gi`
- **THEN** chat pods get an `emptyDir` with `sizeLimit: 4Gi` and task pods are unchanged

#### Scenario: Invalid size limit
- **WHEN** `sandbox.chatDataSizeLimit` is `lots`
- **THEN** the control plane exits at startup with an error naming `sandbox.chatDataSizeLimit`

#### Scenario: Next run starts empty
- **WHEN** a run of `DF-12-CHAT` ended and its pod was deleted, and the next run starts
- **THEN** `/data` of the new pod is empty before the runtime prepares it
