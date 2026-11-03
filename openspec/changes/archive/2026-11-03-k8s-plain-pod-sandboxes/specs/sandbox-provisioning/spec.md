# Spec Delta

## MODIFIED Requirements

### Requirement: Sandbox per run
The control plane SHALL create one bare Pod per run (`restartPolicy: Never`) in the sandbox namespace with: the project image; entrypoint `/opt/factory/factory-agent run`; the runtime image read-only at `/opt/factory`, the run's harness image at `/opt/harness/<harness>` unless the project sets an adapter command; the task PVC at `/data`; a memory volume at `/run/factory`; a cert-manager csi volume with the per-pod URI SAN; labels for instance, run, task, project and persona.

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

### Requirement: Teardown keeps the volume
Tearing down a run SHALL delete its Pod (and with it the network policy) and SHALL keep the task PVC. Teardown of an already deleted Pod SHALL succeed.

#### Scenario: Run ended
- **WHEN** the run of `DF-1` is torn down
- **THEN** its pod and network policy are gone and PVC `task-df-1` still exists

#### Scenario: Already deleted
- **WHEN** a run whose pod no longer exists is torn down
- **THEN** the call succeeds

### Requirement: Sandbox events
The provider SHALL report when a run's pod is running with its pod IP, and when a run's pod is deleted, failed or completed, or cannot start its container, including the reason.

#### Scenario: Pod started
- **WHEN** the run's pod becomes Running with IP 10.0.3.7
- **THEN** a `Started` event with the run id and 10.0.3.7 is emitted

#### Scenario: Image pull fails
- **WHEN** the project image can't be pulled
- **THEN** a `Gone` event with a reason naming the image pull failure is emitted

#### Scenario: Container exits without a report
- **WHEN** the agent container of a run exits with code 0 and the pod reaches phase `Succeeded`
- **THEN** a `Gone` event with a reason naming the completed pod is emitted

### Requirement: Startup verification
At startup the control plane SHALL verify that the configured sandbox namespace exists and that it holds the permissions it needs there, and SHALL refuse to start with a message naming each missing item. It SHALL never create the namespace.

#### Scenario: Missing permission
- **WHEN** the control plane may not create `persistentvolumeclaims` in the sandbox namespace
- **THEN** it exits non-zero with a message naming that permission

#### Scenario: Missing pod permission
- **WHEN** the control plane may not create `pods` in the sandbox namespace
- **THEN** it exits non-zero with a message naming that permission

#### Scenario: No agent-sandbox permissions required
- **WHEN** the control plane has no permissions on `agents.x-k8s.io` resources
- **THEN** startup verification passes
