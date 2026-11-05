## MODIFIED Requirements

### Requirement: Harness start
The runtime SHALL write the harness settings from `Assign` under `/data/home` (overwriting what's there) and start the adapter command of `Assign` (the project's, otherwise the harness image's default) through its sandbox subcommand, with cwd `/data/repo`, `HOME=/data/home`, the pod's proxy environment and the `Assign` env: gateway base URL, placeholder key, CA variables and, for `claude-code`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`. It SHALL NOT advertise ACP file-system or terminal client capabilities.

#### Scenario: Adapter from the harness image
- **WHEN** a run of a project without an adapter command starts
- **THEN** the runtime starts `/opt/harness/claude-code/bin/claude-agent-acp` through `factory-agent sandbox --`

#### Scenario: Adapter from the project image
- **WHEN** a run of a project with adapter command `/opt/harness/bin/fake-acp-agent` starts
- **THEN** the runtime starts that command through `factory-agent sandbox --`

#### Scenario: Claude Code sends no non-essential traffic
- **WHEN** a run of the `claude-code` harness starts, with a subscription or an API key provider
- **THEN** the harness environment contains `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, so its telemetry and update checks do not bypass the inference endpoint

#### Scenario: Model traffic bypasses the egress proxy
- **WHEN** the harness calls its base URL
- **THEN** the request goes directly to the gateway's inference endpoint, not through the CONNECT proxy
