## ADDED Requirements

### Requirement: OpenCode harness image
The repository SHALL provide a harness image for the `opencode` harness with no base (`scratch`) for amd64 and arm64 that, used as an image volume, holds an executable launcher at `bin/opencode` and the pinned OpenCode release binary with the libraries it needs, so that `opencode acp` runs in a project image without them. The release archive SHALL be checksum-verified at build time.

#### Scenario: ACP from the volume
- **WHEN** the image is mounted at `/opt/harness/opencode` in a project image without musl
- **THEN** `/opt/harness/opencode/bin/opencode acp` completes an ACP `initialize` handshake and a `session/new`

#### Scenario: Version copies agree
- **WHEN** the OpenCode version in the image build differs from the default image tag in the instance configuration, the chart values or the reference documentation
- **THEN** the version check fails
