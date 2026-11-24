## ADDED Requirements

### Requirement: Sidecar containers in the pod
The pod of a run with sidecars SHALL carry each sidecar of the run's snapshot as an init container with `restartPolicy: Always`, in the snapshot's order, with its image, command, args, env, TCP ports, resources, a startup probe from its start probe (period 2 s, failure threshold from the start timeout) and no readiness or liveness probe. The pod's `restartPolicy` SHALL stay `Never` and it SHALL NOT share the process namespace. A run without sidecars SHALL get a pod without init containers.

#### Scenario: Pod with a Playwright sidecar
- **WHEN** run 1 of `DF-7` is provisioned with sidecar `playwright` (port 8931, start timeout 60 s)
- **THEN** pod `run-df-7-1` has init container `playwright` with `restartPolicy: Always` and a TCP startup probe on 8931 with period 2 s and failure threshold 30, no `shareProcessNamespace`, and `restartPolicy: Never` on the pod

#### Scenario: Pod without sidecars
- **WHEN** run 1 of `DF-1` is provisioned and project `DF` has no sidecars
- **THEN** pod `run-df-1-1` has no init containers

#### Scenario: Sidecar resources reach the pod
- **WHEN** sidecar `playwright` has requests `500m`/`1Gi` and limits `2`/`2Gi`
- **THEN** init container `playwright` carries exactly these requests and limits, and the agent container keeps `sandbox.resources`

### Requirement: Proxy variables and loopback
A sidecar with egress `proxy` SHALL get the agent container's proxy variables; a sidecar with egress `none` SHALL get none. The `NO_PROXY` and `no_proxy` values of the agent container SHALL contain `localhost`, `127.0.0.1` and `::1`, in addition to the existing entries.

#### Scenario: Proxy variables only with egress proxy
- **WHEN** the run has sidecar `playwright` with egress `proxy` and sidecar `postgres` with egress `none`
- **THEN** `playwright` has the agent container's `HTTP_PROXY`, `HTTPS_PROXY` and `NO_PROXY` values and their lower-case forms, and `postgres` has none of them

#### Scenario: Loopback in NO_PROXY
- **WHEN** any run is provisioned
- **THEN** the agent container's `NO_PROXY` contains `localhost`, `127.0.0.1` and `::1`

### Requirement: Sidecar volumes
A sidecar with a data path SHALL mount an `emptyDir` there, with its ephemeral storage limit as size limit when set. When a sidecar has the shared directory on, the pod SHALL have one `emptyDir` with size limit `sandbox.sharedDirSizeLimit` (default `1Gi`), mounted at `/tmp/shared` in the agent container and in every such sidecar. Without such a sidecar the pod SHALL have no shared volume.

#### Scenario: Data directory with a size limit
- **WHEN** sidecar `postgres` has data path `/var/lib/postgresql/data` and ephemeral storage limit `2Gi`
- **THEN** init container `postgres` mounts an `emptyDir` with size limit `2Gi` at that path

#### Scenario: Shared volume
- **WHEN** the run has sidecar `playwright` with the shared directory on
- **THEN** the pod has one `emptyDir` with size limit `1Gi`, mounted at `/tmp/shared` in `agent` and in `playwright`

#### Scenario: No shared volume
- **WHEN** the run's only sidecar `postgres` has the shared directory off
- **THEN** neither `agent` nor `postgres` mounts `/tmp/shared`

### Requirement: Restricted sidecar containers
Every sidecar container SHALL have `allowPrivilegeEscalation: false` and all capabilities dropped, and SHALL inherit the pod's non-root user and group 1000, `fsGroup` 1000 and the `RuntimeDefault` seccomp profile, so the pod passes the `restricted` Pod Security Standard. Sidecars SHALL NOT mount the runtime image, a harness image, the certificate volume, the `/run/factory` memory volume or the task volume.

#### Scenario: Sidecar security context
- **WHEN** run 1 of `DF-7` is provisioned with sidecar `postgres`
- **THEN** init container `postgres` has `allowPrivilegeEscalation: false`, capabilities `drop: [ALL]` and none of the volumes `factory-runtime`, `tls`, `tmpfs` or `task-data`

### Requirement: Sidecar start events
While the agent container of a run has not started, the provider SHALL report a `Gone` event with a reason starting `sidecar <name> failed to start: ` when a sidecar's container can't be created (`CreateContainerConfigError`, `CreateContainerError`) or when it terminated once (restart count 1 or more, reason and exit code of the termination). A failed startup probe SHALL add `; start probe failed`. Later restarts SHALL NOT produce a `Gone` event.

#### Scenario: Killed by the startup probe
- **WHEN** the kubelet kills sidecar `playwright` after its startup probe failed and its restart count becomes 1
- **THEN** a `Gone` event with reason `sidecar playwright failed to start: Error (exit code 137); start probe failed` is emitted

#### Scenario: Missing config
- **WHEN** init container `postgres` waits with reason `CreateContainerConfigError`
- **THEN** a `Gone` event with a reason starting `sidecar postgres failed to start: CreateContainerConfigError` is emitted

#### Scenario: Restart after the start
- **WHEN** sidecar `playwright` is restarted while the agent container is running
- **THEN** no `Gone` event is emitted

### Requirement: Sidecar image pull events
While the agent container of a run has not started, a sidecar image that can't be pulled (waiting `ErrImagePull`, `ImagePullBackOff` or `InvalidImageName`, or a kubelet pull failure event for that image) SHALL produce a `Gone` event with reason `sidecar <name>: image pull failed: <image>`, also when the kubelet event arrives before the pod status.

#### Scenario: Sidecar image pull failure
- **WHEN** init container `postgres` waits with reason `ImagePullBackOff`
- **THEN** a `Gone` event with reason `sidecar postgres: image pull failed: <image>` is emitted

#### Scenario: Pull failure event arrives first
- **WHEN** the kubelet event `Failed to pull image "<postgres image>"` arrives before the pod status shows the waiting reason
- **THEN** the first `Gone` event of the run already has the reason `sidecar postgres: image pull failed: <image>`

### Requirement: Pod larger than the quota
Before creating a run's pod, the provider SHALL compare the pod's total CPU and memory requests and limits (agent plus sidecars) with the `hard` values of every `ResourceQuota` in the sandbox namespace. A pod that exceeds one of them on its own SHALL NOT be created; provisioning SHALL fail with a reason naming the resource, the pod's total and the quota value, and SHALL NOT be treated as `capacity`.

#### Scenario: Sidecars too large for the quota
- **WHEN** the quota allows `limits.memory` 64Gi and the agent (4Gi) plus sidecars (64Gi) need 68Gi
- **THEN** no pod is created and the run ends `provisioning_failed` with a reason naming `limits.memory`, 68Gi and 64Gi, and the task is resumed or failed per "Automatic resume after infrastructure failures"

#### Scenario: Quota used by other runs
- **WHEN** the pod fits the quota on its own but other runs use the rest
- **THEN** the pod is rejected by quota admission and the run ends `provisioning_failed` with detail `capacity`, as today

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
- **WHEN** the control plane may not `get` the `pods/log` subresource in the sandbox namespace
- **THEN** it exits non-zero with a message naming that permission

#### Scenario: Missing quota permission
- **WHEN** the control plane may not `list` `resourcequotas` in the sandbox namespace
- **THEN** it exits non-zero with a message naming that permission

#### Scenario: No agent-sandbox permissions required
- **WHEN** the control plane has no permissions on `agents.x-k8s.io` resources
- **THEN** startup verification passes
