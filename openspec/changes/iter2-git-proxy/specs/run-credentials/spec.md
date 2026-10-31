# Spec Delta

## ADDED Requirements

### Requirement: No Git credentials in the sandbox
The control plane SHALL never send a forge credential to a sandbox. The agent contract has no credential request: `CredentialRequest`/`CredentialGrant` are removed and their field numbers (`AgentMessage` 14, `ControlMessage` 16) reserved.

#### Scenario: Sandbox environment
- **WHEN** a run starts
- **THEN** the runtime receives only `HarnessCredentials` and no forge token

## REMOVED Requirements

### Requirement: Git credentials for the project repository only
**Reason**: The Git proxy in the gateway injects the forge credential; sandboxes no longer receive it (F1).
**Migration**: The runtime uses `Assign.workspace.repo_url` (the proxy URL) without credentials. Remove the forge host from egress allowlists. Runtime and control plane must be updated together (pre-MVP, no mixed versions).
