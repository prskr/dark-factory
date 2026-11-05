## ADDED Requirements

### Requirement: OpenCode harness image
The repository SHALL provide a harness image for the `opencode` harness with no base (`scratch`) for amd64 and arm64 that, used as an image volume, holds the pinned OpenCode release binary at `bin/opencode`, with its ELF interpreter and library path set to the musl loader and libraries bundled under `lib/`, so that it starts without a launcher and `opencode acp` runs in a project image without them. The release archive SHALL be checksum-verified at build time.

#### Scenario: Interpreter in the volume
- **WHEN** the image is inspected
- **THEN** `bin/opencode` requests the interpreter `/opt/harness/opencode/lib/ld-musl-<arch>.so.1` and every library it needs is in `lib/`

#### Scenario: ACP from the volume
- **WHEN** the image is mounted at `/opt/harness/opencode` in a project image without musl
- **THEN** `/opt/harness/opencode/bin/opencode acp` completes an ACP `initialize` handshake and a `session/new`

#### Scenario: Version copies agree
- **WHEN** the OpenCode version in the image build differs from the default image tag in the instance configuration, the chart values or the reference documentation
- **THEN** the version check fails

## MODIFIED Requirements

### Requirement: Claude Code harness image
The repository SHALL provide a harness image for the `claude-code` harness with no base (`scratch`) that, used as an image volume, holds the ACP adapter `@agentclientprotocol/claude-agent-acp`, Node.js at `bin/node` and the Claude Code CLI at `bin/claude`, both with their ELF interpreter and library path set to the musl loader and libraries bundled under `lib/`, so that the adapter runs in a project image without Node.js. The image SHALL contain no launcher.

#### Scenario: Adapter from the volume
- **WHEN** the image is mounted at `/opt/harness/claude-code` in a project image that has no Node.js
- **THEN** `/opt/harness/claude-code/bin/node /opt/harness/claude-code/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js` completes an ACP `initialize` handshake

#### Scenario: Alpine and glibc project images
- **WHEN** the image is mounted in an Alpine and in a Debian project image
- **THEN** the handshake succeeds in both

#### Scenario: Interpreter in the volume
- **WHEN** the image is inspected
- **THEN** `bin/node` and `bin/claude` request the interpreter `/opt/harness/claude-code/lib/ld-musl-<arch>.so.1` and every library they need is in `lib/`

#### Scenario: Glibc fallback
- **WHEN** the static-musl approach was rejected in the spike (design D2)
- **THEN** the image is built against glibc, and the reference documentation names glibc as a requirement of project images
