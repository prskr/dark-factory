## MODIFIED Requirements

### Requirement: Sandbox events
The provider SHALL report when a run's pod is running with its pod IP, and when a run's pod is deleted, failed or completed, or cannot start its container, including the reason. A pod that cannot start because its runtime or harness image volume cannot be pulled counts as unable to start.

#### Scenario: Pod started
- **WHEN** the run's pod becomes Running with IP 10.0.3.7
- **THEN** a `Started` event with the run id and 10.0.3.7 is emitted

#### Scenario: Image pull fails
- **WHEN** the project image can't be pulled
- **THEN** a `Gone` event with a reason naming the image pull failure is emitted

#### Scenario: Harness image cannot be pulled
- **WHEN** the harness image `registry.example.org/claude-harness:missing` of a run's pod can't be pulled
- **THEN** a `Gone` event with a reason naming the image pull failure and the image is emitted within 30 seconds of the kubelet reporting the failure, and the run doesn't wait for the connect timeout

#### Scenario: Runtime image cannot be pulled
- **WHEN** the runtime image of a run's pod can't be pulled
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

#### Scenario: Missing event permission
- **WHEN** the control plane may not `watch` `events` in the sandbox namespace
- **THEN** it exits non-zero with a message naming that permission

#### Scenario: No agent-sandbox permissions required
- **WHEN** the control plane has no permissions on `agents.x-k8s.io` resources
- **THEN** startup verification passes
