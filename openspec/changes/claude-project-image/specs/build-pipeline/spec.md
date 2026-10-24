# Spec Delta

## ADDED Requirements

### Requirement: Reference Claude Code project image
The repository SHALL provide a reference Claude Code project image that contains the ACP adapter `@agentclientprotocol/claude-agent-acp` executable at `/opt/harness/bin/claude-agent-acp`, git, bash, CA certificates and a Node.js version the adapter supports, and runs as UID 1000 with `/workspace` as working directory.

#### Scenario: Adapter at the default path
- **WHEN** a sandbox uses the reference image with the default `harness.claudeCode.adapterCommand`
- **THEN** the runtime finds an executable adapter at `/opt/harness/bin/claude-agent-acp`

#### Scenario: Non-root user
- **WHEN** the image is inspected
- **THEN** its configured user is UID 1000

### Requirement: Reference image is checked and published
CI SHALL build the reference image and run its structure check for every pull request that changes it; a release SHALL push it for `linux/amd64` and `linux/arm64` to `ghcr.io/prskr/dark-factory/claude-project`, tagged with the adapter version.

#### Scenario: Broken image in a pull request
- **WHEN** a pull request changes the Dockerfile so that `/opt/harness/bin/claude-agent-acp` is missing
- **THEN** the CI job fails

#### Scenario: Release
- **WHEN** a release tag is pushed
- **THEN** `ghcr.io/prskr/dark-factory/claude-project:<adapter version>` exists for amd64 and arm64
