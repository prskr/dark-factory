# Spec Delta

## Purpose

Defines the only credentials a run receives — the harness subscription token and, in v1, a Git credential for the project's repository — and how they are kept out of persistent storage (ADR-0008, ADR-0010).

## ADDED Requirements

### Requirement: Harness credential delivery
After every `HelloAck`, the control plane SHALL send `HarnessCredentials` with the persona's harness credential resolved from its Kubernetes Secret reference (`claude-code`: env `CLAUDE_CODE_OAUTH_TOKEN`). Secret values SHALL never be stored in the database, written to logs or put in the pod spec.

#### Scenario: Claude token
- **WHEN** a run for persona "Linus" (credential → Secret `claude`, key `token`) connects
- **THEN** the runtime receives `HarnessCredentials.env.CLAUDE_CODE_OAUTH_TOKEN` with the Secret's value

#### Scenario: Missing Secret
- **WHEN** the referenced Secret or key doesn't exist
- **THEN** the run fails with detail naming the credential reference (not its value)

### Requirement: Git credentials for the project repository only
A `CredentialRequest` for Git SHALL be granted with the project's forge credential only if protocol, host and path match the project's repository URL; any other request SHALL be denied with a reason.

#### Scenario: Request for the project repo
- **WHEN** the runtime requests credentials for `https://git.example.org/peter/dark-factory.git`
- **THEN** it receives a `GitCredential` with the bot username and token

#### Scenario: Request for another repo
- **WHEN** the runtime requests credentials for `https://git.example.org/peter/other.git`
- **THEN** the grant contains `denied_reason` and no credential
