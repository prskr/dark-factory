# Spec Delta

## ADDED Requirements

### Requirement: No Git credentials in the sandbox
The control plane SHALL answer every `CredentialRequest` for Git with a `denied_reason` stating that repository access goes through the Git proxy, and SHALL never send a forge credential to a sandbox.

#### Scenario: Legacy credential request
- **WHEN** a runtime sends a `CredentialRequest` for `https://git.example.org/peter/dark-factory.git`
- **THEN** the grant contains `denied_reason` and no credential

## REMOVED Requirements

### Requirement: Git credentials for the project repository only
**Reason**: The Git proxy in the gateway injects the forge credential; sandboxes no longer receive it (F1).
**Migration**: The runtime uses `Assign.workspace.repo_url` (the proxy URL) without credentials. Remove the forge host from egress allowlists.
