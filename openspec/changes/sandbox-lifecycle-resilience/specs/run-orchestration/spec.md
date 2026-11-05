## MODIFIED Requirements

### Requirement: Run provisioning
Starting a run SHALL ensure the task volume exists, then provision a sandbox for the run with the project's image and labels for run, task, project and persona, and move the run to `provisioning`, then `connecting` once the sandbox reports its pod IP. A run in `pending` or `provisioning` for more than 1 minute without a sandbox SHALL be provisioned again on the next tick; volume and sandbox creation SHALL be idempotent. When a cluster target of the run is temporarily unresolvable and has no earlier resolution on this replica, the sandbox SHALL NOT be created and the run SHALL stay `provisioning` until a later attempt succeeds or the connect timeout ends it.

#### Scenario: Provisioning fails
- **WHEN** the provider rejects the sandbox for a reason other than capacity or a temporarily unresolvable cluster target (e.g. invalid image reference)
- **THEN** the run ends with outcome `provisioning_failed` and the task is resumed or failed per "Automatic resume"

#### Scenario: Quota exceeded
- **WHEN** the provider rejects the sandbox because a quota is exceeded
- **THEN** the run ends `provisioning_failed` with detail `capacity`, the task becomes `queued` with a resume time 2 minutes ahead, and the resume count is unchanged

#### Scenario: Provisioning interrupted
- **WHEN** a replica stops after creating run 3 of `DF-7` and before creating its Sandbox
- **THEN** another tick provisions the run within 2 minutes and the run connects normally

#### Scenario: Cluster target temporarily unresolvable
- **WHEN** run 1 of `DF-1` is provisioned, project `DF` has cluster target `nativelink/scheduler`, the Service lookup times out and no earlier lookup of that target succeeded
- **THEN** no pod is created for the run, the run stays `provisioning`, and a warning naming `nativelink/scheduler` is logged

#### Scenario: Cluster target resolves on a later attempt
- **WHEN** the lookup of `nativelink/scheduler` succeeds on a later attempt within the connect timeout
- **THEN** the pod is created with the `hostAliases` entry for `scheduler.nativelink.svc` and the run connects normally

#### Scenario: Cluster target never resolves
- **WHEN** the lookup of `nativelink/scheduler` keeps failing until the connect timeout
- **THEN** the run ends `connect_timeout` and the task is resumed or failed per "Automatic resume after infrastructure failures"

### Requirement: Timeouts and lost sandboxes
The control plane SHALL fail a run that doesn't connect within the configured connect timeout (outcome `connect_timeout`), SHALL send `Cancel` when the wall-clock budget plus grace is exceeded, and SHALL fail a non-terminal run whose sandbox disappears (outcome `sandbox_lost`), including a sandbox that disappeared while no control plane replica was running. Failed runs' sandboxes SHALL be torn down.

#### Scenario: Sandbox never connects
- **WHEN** a sandbox doesn't open the stream within the connect timeout
- **THEN** the run fails with `connect_timeout`, the sandbox is deleted and the task is resumed or failed per "Automatic resume after infrastructure failures"

#### Scenario: Pod deleted mid-run
- **WHEN** the sandbox pod of a running run is deleted
- **THEN** the run fails with `sandbox_lost` and the task volume is kept

#### Scenario: Pod deleted while the control plane was down
- **WHEN** the sandbox pod of running run 2 of `DF-4` is deleted while no control plane replica runs, and a replica starts afterwards
- **THEN** within one minute of the replica's start the run fails with `sandbox_lost` with a reason stating that the sandbox pod is gone, and the task is resumed or failed per "Automatic resume after infrastructure failures"

#### Scenario: Pod still there after a restart
- **WHEN** a replica starts while the sandbox pod of running run 2 of `DF-4` still exists
- **THEN** the run is not ended
