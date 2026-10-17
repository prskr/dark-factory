# Spec Delta

## Purpose

Runs an external agent (Claude Code first) through the Agent Client Protocol for one run: configuring and starting it, feeding prompts turn by turn, translating its output into the Dark Factory transcript and deciding how the run ends (ADR-0005, ADR-0017).

## ADDED Requirements

### Requirement: Harness start
The runtime SHALL write the harness settings from `Assign` under `/data/home` (overwriting what's there), start the adapter command with cwd `/data/repo`, `HOME=/data/home`, the pod's proxy environment, `Assign` env and `HarnessCredentials` env, and SHALL NOT advertise ACP file-system or terminal client capabilities. Harness credentials SHALL never be written to `/data`.

#### Scenario: Token stays off the volume
- **WHEN** the harness runs with `CLAUDE_CODE_OAUTH_TOKEN`
- **THEN** no file under `/data` contains the token value

### Requirement: Session continuity
With `Assign.continuation.harness_session_id` set, the runtime SHALL resume via `session/load`; if loading fails or isn't supported, it SHALL start a new session and prepend `continuation.summary` to the first prompt. The factory MCP server SHALL be registered in every session.

#### Scenario: Resume
- **WHEN** run 2 starts with session id S and the adapter supports `session/load`
- **THEN** the session S is loaded and the follow-up message is sent as the first prompt

#### Scenario: Resume fails
- **WHEN** `session/load` fails
- **THEN** a new session is created and the first prompt starts with the summary; a `harness.session_resume_failed` notice is emitted

### Requirement: Turn-based prompts
All prompts the runtime holds when a turn starts SHALL form that turn's input. For each, a `UserMessage` with its prompt id SHALL be emitted at turn start. Prompts arriving during a turn SHALL wait for the next turn.

#### Scenario: Two queued messages
- **WHEN** two prompts arrive during a turn
- **THEN** the next turn's input contains both and `TurnStarted.prompt_ids` lists both

### Requirement: Permission requests auto-approved
Harness permission requests SHALL be answered with an allow option.

#### Scenario: Shell command permission
- **WHEN** the harness asks permission to run a shell command
- **THEN** the runtime approves it without involving the control plane

### Requirement: Transcript translation
The runtime SHALL translate ACP updates into transcript v1 events: agent text and thought chunks with a message id that changes whenever text resumes after another event, tool calls and updates (origin `FACTORY` for the factory server's tools, raw input/output truncated above 64 KiB with `truncated` set), plans, and `TurnStarted`/`TurnEnded` with stop reason and per-model usage.

#### Scenario: Text after a tool call
- **WHEN** the agent writes text, calls a tool, then writes more text
- **THEN** the two text parts carry different message ids

#### Scenario: Large tool output
- **WHEN** a tool returns 200 KiB of output
- **THEN** the event contains at most 64 KiB of it and `truncated: true`

### Requirement: Usage capture
`TurnEnded.usage` SHALL contain input, output, cache-read and cache-write tokens per model, from the harness if it reports them (`USAGE_SOURCE_HARNESS`), otherwise parsed from the harness's session files (`USAGE_SOURCE_SESSION_LOG`).

#### Scenario: Adapter without usage
- **WHEN** the adapter reports no usage for a turn
- **THEN** usage is read from the Claude Code session log for that turn

### Requirement: Run outcome
After each turn the runtime SHALL continue with queued prompts if any; otherwise it SHALL finalize with outcome: `COMPLETED` if a tool result requested ending the run; `RATE_LIMITED` (with retry time if known) if the provider rate-limited; `LIMIT_REACHED` if a budget or max-token/turn stop reason was hit; `CANCELLED` on `Cancel`; `FAILED` if the harness crashed; `END_TURN` otherwise.

#### Scenario: Agent asks a question in text
- **WHEN** the turn ends normally and no prompts are queued
- **THEN** the run finalizes with `END_TURN`

#### Scenario: complete_task called
- **WHEN** a `ToolCallResult` with `end_run_after_turn` arrived during the turn
- **THEN** after the turn the run finalizes with `COMPLETED`, even if prompts are queued

#### Scenario: Subscription limit
- **WHEN** the harness reports a provider rate limit
- **THEN** `TurnEnded.stop_reason` is `RATE_LIMITED` and the run finalizes with `RUN_OUTCOME_RATE_LIMITED`

### Requirement: Budgets enforced locally
The runtime SHALL enforce `Assign.budgets` (wall clock, output tokens, turns) by cancelling the current turn and finalizing with `LIMIT_REACHED`.

#### Scenario: Turn limit
- **WHEN** `max_turns` is 3 and a fourth turn would start
- **THEN** the run finalizes with `LIMIT_REACHED` instead
