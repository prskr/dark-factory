# Spec Delta

## MODIFIED Requirements

### Requirement: Harness start
The runtime SHALL write the harness settings from `Assign` under `/data/home` (overwriting what's there), start the adapter command of `Assign` (the project's adapter command if it sets one, otherwise the harness default, by default `/opt/harness/claude-code/bin/claude-agent-acp` from the harness image volume) with cwd `/data/repo`, `HOME=/data/home`, the pod's proxy environment, `Assign` env and `HarnessCredentials` env, and SHALL NOT advertise ACP file-system or terminal client capabilities. Harness credentials SHALL never be written to `/data`.

#### Scenario: Token stays off the volume
- **WHEN** the harness runs with `CLAUDE_CODE_OAUTH_TOKEN`
- **THEN** no file under `/data` contains the token value

#### Scenario: Adapter from the harness image
- **WHEN** a run of a project without an adapter command starts
- **THEN** the runtime starts `/opt/harness/claude-code/bin/claude-agent-acp`

#### Scenario: Adapter from the project image
- **WHEN** a run of a project with adapter command `/opt/harness/bin/fake-acp-agent` starts
- **THEN** the runtime starts that command
