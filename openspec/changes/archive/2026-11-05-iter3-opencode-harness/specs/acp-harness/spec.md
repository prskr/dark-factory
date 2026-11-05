## ADDED Requirements

### Requirement: Harness files
The runtime SHALL write every file of `Assign.harness.acp.files` below `/data/home` before the harness starts, overwriting what is there, creating missing directories, without following symlinks. A path that is absolute or contains `..` SHALL fail the run before the harness starts. The runtime SHALL write no other harness file of its own.

#### Scenario: Files written
- **WHEN** `Assign` carries files `.claude/settings.json` and `.claude/CLAUDE.md`
- **THEN** both exist under `/data/home` with the given content before the adapter starts

#### Scenario: Path escapes HOME
- **WHEN** `Assign` carries a file `../repo/.git/config`
- **THEN** the run fails before the adapter starts and no file is written outside `/data/home`

#### Scenario: Symlink planted by the agent
- **WHEN** `/data/home/.config` is a symlink to `/data/repo` and `Assign` carries `.config/opencode/AGENTS.md`
- **THEN** the runtime does not write through the symlink

### Requirement: System prompt per harness
The control plane SHALL pass the persona's system prompt as a harness file at the harness's prompt path: `.claude/CLAUDE.md` for `claude-code`, `.config/opencode/AGENTS.md` for `opencode`. For `claude-code` a non-empty `harness.claudeCode.settingsJson` SHALL be passed as `.claude/settings.json`.

#### Scenario: OpenCode system prompt
- **WHEN** a run starts for an `opencode` persona with a system prompt
- **THEN** `Assign` carries `.config/opencode/AGENTS.md` with that prompt and no `.claude` file

#### Scenario: Claude Code files
- **WHEN** a run starts for a `claude-code` persona with a system prompt and `harness.claudeCode.settingsJson` is set
- **THEN** `Assign` carries `.claude/CLAUDE.md` with the prompt and `.claude/settings.json` with the settings

## MODIFIED Requirements

### Requirement: Harness start
The runtime SHALL write the harness files from `Assign` under `/data/home` and start the adapter command of `Assign` (the project's, otherwise the harness image's default) with cwd `/data/repo`, `HOME=/data/home`, the pod's proxy environment and the `Assign` env: the inference configuration of the harness (base URL and placeholder key variables, or a configuration document), CA variables and the harness's fixed variables, for `claude-code` `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, `ANTHROPIC_MODEL` set to the persona's model ID and, when the harness image is mounted, `CLAUDE_CODE_EXECUTABLE=/opt/harness/claude-code/bin/claude`. It SHALL NOT advertise ACP file-system or terminal client capabilities.

#### Scenario: Adapter from the harness image
- **WHEN** a run of a project without an adapter command starts
- **THEN** the runtime starts `/opt/harness/claude-code/bin/node /opt/harness/claude-code/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js`

#### Scenario: OpenCode adapter from the harness image
- **WHEN** a run of an `opencode` persona in a project without an adapter command starts
- **THEN** the runtime starts `/opt/harness/opencode/bin/opencode acp`

#### Scenario: Adapter from the project image
- **WHEN** a run of a project with adapter command `/opt/harness/bin/fake-acp-agent` starts
- **THEN** the runtime starts that command, and for `claude-code` the env has no `CLAUDE_CODE_EXECUTABLE`

#### Scenario: Claude Code sends no non-essential traffic
- **WHEN** a run of the `claude-code` harness starts, with a subscription or an API key provider
- **THEN** the harness environment contains `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, so its telemetry and update checks do not bypass the inference endpoint

#### Scenario: Model traffic bypasses the egress proxy
- **WHEN** the harness calls its base URL
- **THEN** the request goes directly to the gateway's inference endpoint, not through the CONNECT proxy

#### Scenario: Claude Code uses the persona's model
- **WHEN** a run of a `claude-code` persona with model `claude-sonnet-5-5` starts
- **THEN** the harness environment contains `ANTHROPIC_MODEL=claude-sonnet-5-5`, so Claude Code sends that model to the inference endpoint instead of its default
