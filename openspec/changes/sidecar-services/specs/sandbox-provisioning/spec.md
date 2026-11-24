## ADDED Requirements

### Requirement: Sidecar containers in the pod
The pod of a run with sidecars SHALL carry each sidecar of the run's snapshot as an init container with `restartPolicy: Always`, in the snapshot's order, with its image, command, args, env, TCP ports, resources, a startup probe from its start probe (period 2 s, failure threshold from the start timeout) and no readiness or liveness probe. The pod's `restartPolicy` SHALL stay `Never`. A run without sidecars SHALL get a pod without init containers.

#### Scenario: Pod with a Playwright sidecar
- **WHEN** run 1 of `DF-7` is provisioned with sidecar `playwright` (port 8931, shared directory on)
- **THEN** pod `run-df-7-1` has init container `playwright` with `restartPolicy: Always`, a TCP startup probe on 8931, the volume mounted at `/tmp/shared` in `playwright` and in `agent`, and `restartPolicy: Never` on the pod

#### Scenario: Pod without sidecars
- **WHEN** run 1 of `DF-1` is provisioned and project `DF` has no sidecars
- **THEN** pod `run-df-1-1` has no init containers and no `/tmp/shared` mount

#### Scenario: Sidecar resources reach the pod
- **WHEN** sidecar `playwright` has requests `500m`/`1Gi` and limits `2`/`2Gi`
- **THEN** init container `playwright` carries exactly these requests and limits, and the agent container keeps `sandbox.resources`

### Requirement: Sidecar volumes
A sidecar with a data path SHALL mount an `emptyDir` there, with its ephemeral storage limit as size limit when set. When a sidecar has the shared directory on, the pod SHALL have one `emptyDir` with size limit `sandbox.sharedDirSizeLimit` (default `1Gi`), mounted at `/tmp/shared` in the agent container and in every such sidecar. Without such a sidecar the pod SHALL have no shared volume.

#### Scenario: Data directory with a size limit
- **WHEN** sidecar `postgres` has data path `/var/lib/postgresql/data` and ephemeral storage limit `2Gi`
- **THEN** init container `postgres` mounts an `emptyDir` with size limit `2Gi` at that path

#### Scenario: No shared volume
- **WHEN** the run's only sidecar `postgres` has the shared directory off
- **THEN** neither `agent` nor `postgres` mounts `/tmp/shared`

### Requirement: Restricted sidecar containers
Every sidecar container SHALL have `allowPrivilegeEscalation: false` and all capabilities dropped, and SHALL inherit the pod's non-root user and group 1000 and the `RuntimeDefault` seccomp profile, so the pod passes the `restricted` Pod Security Standard. Sidecars SHALL NOT mount the runtime image, a harness image, the certificate volume, the `/run/factory` memory volume or the task volume.

#### Scenario: Sidecar security context
- **WHEN** run 1 of `DF-7` is provisioned with sidecar `postgres`
- **THEN** init container `postgres` has `allowPrivilegeEscalation: false`, capabilities `drop: [ALL]` and none of the volumes `factory-runtime`, `tls`, `tmpfs` or `task-data`

### Requirement: Sidecar events
While the agent container of a run has not started, the provider SHALL report a `Gone` event when a sidecar can't pull its image (reason `sidecar <name>: image pull failed: <image>`) and when a sidecar restarted 3 times or is in back-off (reason `sidecar <name> failed to start: ` followed by the last termination reason and exit code, or `start probe failed`). After the agent container started, sidecar restarts SHALL NOT produce a `Gone` event.

#### Scenario: Sidecar image pull failure
- **WHEN** init container `postgres` waits with reason `ImagePullBackOff`
- **THEN** a `Gone` event with reason `sidecar postgres: image pull failed: <image>` is emitted

#### Scenario: Restart after the start
- **WHEN** sidecar `playwright` is restarted while the agent container is running
- **THEN** no `Gone` event is emitted

## MODIFIED Requirements

### Requirement: Startup verification
At startup the control plane SHALL verify that the configured sandbox namespace exists and that it holds the permissions it needs there, and SHALL refuse to start with a message naming each missing item. It SHALL never create the namespace.

#### Scenario: Missing permission
- **WHEN** the control plane may not create `persistentvolumeclaims` in the sandbox namespace
- **THEN** it exits non-zero with a message naming that permission

#### Scenario: Missing pod permission
- **WHEN** the control plane may not create `pods` in the sandbox namespace
- **THEN** it exits non-zero with a message naming that permission

#### Scenario: Missing event permission
- **WHEN** the control plane may not `watch` `events` in the sandbox namespace
- **THEN** it exits non-zero with a message naming that permission

#### Scenario: Missing pod log permission
- **WHEN** the control plane may not `get` `pods/log` in the sandbox namespace
- **THEN** it exits non-zero with a message naming that permission

#### Scenario: No agent-sandbox permissions required
- **WHEN** the control plane has no permissions on `agents.x-k8s.io` resources
- **THEN** startup verification passes
