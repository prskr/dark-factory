## MODIFIED Requirements

### Requirement: Harness start
The runtime SHALL write the harness settings from `Assign` under `/data/home` (overwriting what's there), start the adapter command of `Assign` (the project's adapter command if it sets one, otherwise the harness default, by default `/opt/harness/claude-code/bin/claude-agent-acp` from the harness image volume) with cwd `/data/repo`, `HOME=/data/home`, the pod's proxy environment and `Assign` env (including the gateway base URL, placeholder key and CA variables; for `claude-code` also `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`), and SHALL NOT advertise ACP file-system or terminal client capabilities.

#### Scenario: Adapter from the harness image
- **WHEN** a run of a project without an adapter command starts
- **THEN** the runtime starts `/opt/harness/claude-code/bin/claude-agent-acp`

#### Scenario: Adapter from the project image
- **WHEN** a run of a project with adapter command `/opt/harness/bin/fake-acp-agent` starts
- **THEN** the runtime starts that command

#### Scenario: Claude Code sends no non-essential traffic
- **WHEN** a run of the `claude-code` harness starts, with a subscription or an API key provider
- **THEN** the harness environment contains `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, so its telemetry and update checks do not bypass the inference endpoint

#### Scenario: Model traffic bypasses the egress proxy
- **WHEN** the harness calls its base URL
- **THEN** the request goes directly to the gateway's inference endpoint, not through the CONNECT proxy

### Requirement: Transcript translation
The runtime SHALL translate ACP updates into transcript v1 events: agent text and thought chunks with a message id that changes whenever text resumes after another event, tool calls and updates (origin `FACTORY` for the factory server's tools, raw input/output truncated above 64 KiB with `truncated` set), plans, and `TurnStarted`/`TurnEnded` with stop reason.

#### Scenario: Text after a tool call
- **WHEN** the agent writes text, calls a tool, then writes more text
- **THEN** the two text parts carry different message ids

#### Scenario: Large tool output
- **WHEN** a tool returns 200 KiB of output
- **THEN** the event contains at most 64 KiB of it and `truncated: true`

### Requirement: Budgets enforced locally
The runtime SHALL enforce `Assign.budgets` (wall clock, turns) by cancelling the current turn and finalizing with `LIMIT_REACHED`.

#### Scenario: Turn limit
- **WHEN** `max_turns` is 3 and a fourth turn would start
- **THEN** the run finalizes with `LIMIT_REACHED` instead

## REMOVED Requirements

### Requirement: Harness credentials stay off the volume
**Reason**: The sandbox no longer receives harness credentials (ADR-0050); the placeholder key is not a secret.
**Migration**: None.

### Requirement: Usage capture
**Reason**: Usage is measured only by the gateway (ADR-0050).
**Migration**: Usage comes from gateway usage records (`model-usage`); `TurnEnded.usage`, `usage_source` and `UsageSource` are removed. The output-token budget (`Budgets.max_output_tokens`, never set by the control plane) is removed; token budgets move to the gateway (backlog).
