# run-credentials Specification

## Purpose

Defines the only credentials a run receives — the harness subscription token and, in v1, a Git credential for the project's repository — and how they are kept out of persistent storage (ADR-0008, ADR-0010).

## Requirements

### Requirement: Harness credential delivery
After every `HelloAck`, the control plane SHALL send `HarnessCredentials` with the persona's harness credential resolved from its Kubernetes Secret reference (`claude-code`: env `CLAUDE_CODE_OAUTH_TOKEN`). Secret values SHALL never be stored in the database, written to logs or put in the pod spec.

#### Scenario: Claude token
- **WHEN** a run for persona "Linus" (credential → Secret `claude`, key `token`) connects
- **THEN** the runtime receives `HarnessCredentials.env.CLAUDE_CODE_OAUTH_TOKEN` with the Secret's value

#### Scenario: Missing Secret
- **WHEN** the referenced Secret or key doesn't exist
- **THEN** the run fails with detail naming the credential reference (not its value)

### Requirement: No Git credentials in the sandbox
The control plane SHALL never send a forge credential to a sandbox. The agent contract has no credential request: `CredentialRequest`/`CredentialGrant` are removed and their field numbers (`AgentMessage` 14, `ControlMessage` 16) reserved.

#### Scenario: Sandbox environment
- **WHEN** a run starts
- **THEN** the runtime receives only `HarnessCredentials` and no forge token
