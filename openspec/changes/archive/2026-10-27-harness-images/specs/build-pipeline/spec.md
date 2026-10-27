# Spec Delta

## ADDED Requirements

### Requirement: Claude Code harness image
The repository SHALL provide a harness image for the `claude-code` harness with no base (`scratch`) that, used as an image volume, holds an executable adapter launcher at `bin/claude-agent-acp`, the ACP adapter `@agentclientprotocol/claude-agent-acp`, the Node.js runtime it needs and the Claude Code CLI, so that the adapter runs in a project image without Node.js.

#### Scenario: Adapter from the volume
- **WHEN** the image is mounted at `/opt/harness/claude-code` in a project image that has no Node.js
- **THEN** `/opt/harness/claude-code/bin/claude-agent-acp` completes an ACP `initialize` handshake

#### Scenario: Alpine and glibc project images
- **WHEN** the image is mounted in an Alpine and in a Debian project image
- **THEN** the handshake succeeds in both

#### Scenario: Glibc fallback
- **WHEN** the static-musl approach was rejected in the spike (design D2)
- **THEN** the image is built against glibc, and the reference documentation names glibc as a requirement of project images

### Requirement: Reference Claude Code project image
The repository SHALL provide a reference project image with git, bash, curl, CA certificates and OpenSSH client that runs as UID 1000 with `/workspace` as working directory, and contains neither Node.js nor the ACP adapter.

#### Scenario: Non-root user
- **WHEN** the image is inspected
- **THEN** its configured user is UID 1000

#### Scenario: No harness inside
- **WHEN** the image is inspected
- **THEN** `node` and `claude-agent-acp` are absent

### Requirement: Reference images are checked and published
CI SHALL build the harness image and the reference project image and run their structure checks for every pull request that changes either; a release SHALL push both for `linux/amd64` and `linux/arm64` to `ghcr.io/prskr/dark-factory/claude-harness` (tagged with the adapter version) and `ghcr.io/prskr/dark-factory/claude-project`.

#### Scenario: Broken harness image in a pull request
- **WHEN** a pull request changes the Dockerfile so that `bin/claude-agent-acp` is missing or not executable
- **THEN** the CI job fails

#### Scenario: Release
- **WHEN** a release tag is pushed
- **THEN** `claude-harness:<adapter version>` and `claude-project:<release tag>` exist for amd64 and arm64
