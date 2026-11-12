## MODIFIED Requirements

### Requirement: Harness start
The runtime SHALL write the harness files from `Assign` under `/data/home` and start the adapter command of `Assign` (the project's, otherwise the harness image's default) through its sandbox subcommand, with cwd `/data/repo`, `HOME=/data/home`, the pod's proxy environment and the `Assign` env: the harness's inference configuration (variables or a configuration document), CA variables and fixed variables. It SHALL NOT advertise ACP file-system or terminal client capabilities.

#### Scenario: Adapter from the harness image
- **WHEN** a run of a project without an adapter command starts
- **THEN** the runtime starts `/opt/harness/claude-code/bin/node /opt/harness/claude-code/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js` through `factory-agent sandbox --`, with `CLAUDE_CODE_EXECUTABLE=/opt/harness/claude-code/bin/claude` in the env

#### Scenario: OpenCode adapter from the harness image
- **WHEN** a run of an `opencode` persona in a project without an adapter command starts
- **THEN** the runtime starts `/opt/harness/opencode/bin/opencode acp` through `factory-agent sandbox --`

#### Scenario: Adapter from the project image
- **WHEN** a run of a project with adapter command `/opt/harness/bin/fake-acp-agent` starts
- **THEN** the runtime starts that command through `factory-agent sandbox --`, and for `claude-code` the env has no `CLAUDE_CODE_EXECUTABLE`

#### Scenario: Claude Code sends no non-essential traffic
- **WHEN** a run of the `claude-code` harness starts, with a subscription or an API key provider
- **THEN** the harness environment contains `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, so its telemetry and update checks do not bypass the inference endpoint

#### Scenario: Model traffic bypasses the egress proxy
- **WHEN** the harness calls its base URL
- **THEN** the request goes directly to the gateway's inference endpoint, not through the CONNECT proxy

#### Scenario: Claude Code uses the persona's model
- **WHEN** a run of a `claude-code` persona with model `claude-sonnet-5-5` starts
- **THEN** the harness environment contains `ANTHROPIC_MODEL=claude-sonnet-5-5`, so Claude Code sends that model to the inference endpoint instead of its default
