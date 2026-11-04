## ADDED Requirements

### Requirement: Model access through the gateway
At run start the control plane SHALL pick the first wire API of the persona's harness that its model provider offers and SHALL pass the harness the gateway's inference URL as base URL, a fixed placeholder as API key and the instance CA file, in the harness's environment variables, plus the fixed variables the harness needs so that no traffic bypasses the endpoint (`claude-code`: `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`). No model credential SHALL be sent to a sandbox.

#### Scenario: Claude Code with a subscription
- **WHEN** a run for persona "Linus" (`claude-code`, provider with scheme `claude-subscription`) starts
- **THEN** the harness gets `ANTHROPIC_BASE_URL=https://<gateway>:8443/inference`, `CLAUDE_CODE_OAUTH_TOKEN=df-placeholder`, `NODE_EXTRA_CA_CERTS` pointing at the instance CA and `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`

#### Scenario: Claude Code with an API key
- **WHEN** a run for a `claude-code` persona whose provider has scheme `x-api-key` starts
- **THEN** the harness gets `ANTHROPIC_API_KEY=df-placeholder`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` and no `CLAUDE_CODE_OAUTH_TOKEN`

## MODIFIED Requirements

### Requirement: No Git credentials in the sandbox
The control plane SHALL never send a forge credential to a sandbox. The agent contract has no credential request: `CredentialRequest`/`CredentialGrant` are removed and their field numbers (`AgentMessage` 14, `ControlMessage` 16) reserved.

#### Scenario: Sandbox environment
- **WHEN** a run starts
- **THEN** the runtime receives no forge token and no model credential

## REMOVED Requirements

### Requirement: Harness credential delivery
**Reason**: Model credentials stay in the gateway (ADR-0050); `HarnessCredentials` is removed from the agent contract and its field number reserved.
**Migration**: Personas reference a model provider; the harness gets the gateway base URL and a placeholder (see "Model access through the gateway").
