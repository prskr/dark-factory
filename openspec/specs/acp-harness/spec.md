# acp-harness Specification

## Purpose

Runs an external agent (Claude Code first) through the Agent Client Protocol for one run: configuring and starting it, feeding prompts turn by turn, translating its output into the Dark Factory transcript and deciding how the run ends (ADR-0005, ADR-0017).

## Requirements

### Requirement: Harness start
The runtime SHALL write the harness files from `Assign` under `/data/home` and start the adapter command of `Assign` (the project's, otherwise the harness image's default) with cwd `/data/repo`, `HOME=/data/home`, the pod's proxy environment and the `Assign` env: the harness's inference configuration (variables or a configuration document), CA variables and fixed variables. It SHALL NOT advertise ACP file-system or terminal client capabilities.

#### Scenario: Adapter from the harness image
- **WHEN** a run of a project without an adapter command starts
- **THEN** the runtime starts `/opt/harness/claude-code/bin/node /opt/harness/claude-code/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js` with `CLAUDE_CODE_EXECUTABLE=/opt/harness/claude-code/bin/claude` in the env

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

### Requirement: Session continuity
With `Assign.continuation.harness_session_id` set, the runtime SHALL resume via `session/load`; if loading fails or isn't supported, it SHALL start a new session and prepend `continuation.summary` to the first prompt. The factory MCP server SHALL be registered in every session.

#### Scenario: Resume
- **WHEN** run 2 starts with session id S and the adapter supports `session/load`
- **THEN** the session S is loaded and the follow-up message is sent as the first prompt

#### Scenario: Resume fails
- **WHEN** `session/load` fails
- **THEN** a new session is created and the first prompt starts with the summary; a `harness.session_resume_failed` notice is emitted

### Requirement: Turn-based prompts
All prompts the runtime holds when a turn starts SHALL form that turn's input. For each, a `UserMessage` with its prompt id SHALL be emitted at turn start. Prompts arriving during a turn SHALL wait for the next turn. In the turn's input, each prompt SHALL start with a line naming its author (`<author name>:`), in queue order, using the name the control plane sent with the prompt.

#### Scenario: Two queued messages
- **WHEN** two prompts arrive during a turn
- **THEN** the next turn's input contains both and `TurnStarted.prompt_ids` lists both

#### Scenario: Authors in a merged turn
- **WHEN** a turn starts with a message from "Peter Kurfer" followed by a request decision
- **THEN** the harness receives one prompt in which the first message starts with `Peter Kurfer:` and the second with `Request decision:`

#### Scenario: Single message
- **WHEN** a turn starts with one message from "Peter Kurfer"
- **THEN** its text in the harness prompt starts with `Peter Kurfer:`

### Requirement: Permission requests auto-approved
Harness permission requests SHALL be answered with an allow option.

#### Scenario: Shell command permission
- **WHEN** the harness asks permission to run a shell command
- **THEN** the runtime approves it without involving the control plane

### Requirement: Transcript translation
The runtime SHALL translate ACP updates into transcript v1 events: agent text and thought chunks with a message id that changes whenever text resumes after another event, tool calls and updates (origin `FACTORY` for the factory server's tools, raw input/output truncated above 64 KiB with `truncated` set), plans, and `TurnStarted`/`TurnEnded` with stop reason.

#### Scenario: Text after a tool call
- **WHEN** the agent writes text, calls a tool, then writes more text
- **THEN** the two text parts carry different message ids

#### Scenario: Large tool output
- **WHEN** a tool returns 200 KiB of output
- **THEN** the event contains at most 64 KiB of it and `truncated: true`

### Requirement: Run outcome
After each turn the runtime SHALL continue with queued prompts if any; otherwise it SHALL finalize with outcome: `COMPLETED` if a tool result requested ending the run; `RATE_LIMITED` (with retry time if known) if the provider rate-limited; `LIMIT_REACHED` if a budget or max-token/turn stop reason was hit; `CANCELLED` on `Cancel`; `FAILED` if the harness crashed.

#### Scenario: complete_task called
- **WHEN** a `ToolCallResult` with `end_run_after_turn` arrived during the turn
- **THEN** after the turn the run finalizes with `COMPLETED`, even if prompts are queued, and does not wait

#### Scenario: Subscription limit
- **WHEN** the harness reports a provider rate limit
- **THEN** `TurnEnded.stop_reason` is `RATE_LIMITED` and the run finalizes with `RUN_OUTCOME_RATE_LIMITED`

### Requirement: Outcome after a normal turn end
When a turn ended normally and no prompt is queued, the runtime SHALL wait as described in "Idle wait after a normal turn end" and, unless a prompt or `Cancel` arrives, finalize on `Release` with `BLOCKED` if the last turn had a blocking request and `END_TURN` otherwise.

#### Scenario: Agent asks a question in text
- **WHEN** the turn ends normally, no prompts are queued and the control plane sends `Release`
- **THEN** the run finalizes with `END_TURN`

#### Scenario: Nobody answered in time
- **WHEN** the control plane sends `Release` after the deadline of a blocking question passed without a prompt
- **THEN** the run finalizes with `RUN_OUTCOME_BLOCKED`

### Requirement: Budgets enforced locally
The runtime SHALL enforce `Assign.budgets` (wall clock, turns) by cancelling the current turn and finalizing with `LIMIT_REACHED`.

#### Scenario: Turn limit
- **WHEN** `max_turns` is 3 and a fourth turn would start
- **THEN** the run finalizes with `LIMIT_REACHED` instead

### Requirement: Idle wait after a normal turn end
After a turn that ended normally with no queued prompts, the runtime SHALL report `RUN_PHASE_AWAITING_INPUT` and wait with the harness session kept up. A `Prompt` SHALL start the next turn in the same session, `Cancel` SHALL finalize with `CANCELLED`, and `Release` or the end of the wall-clock budget SHALL finalize with `END_TURN`. The runtime SHALL NOT end the wait on its own timer otherwise.

#### Scenario: Follow-up while idle
- **WHEN** a prompt arrives 2 minutes after a normal turn end
- **THEN** the next turn starts in the same harness session without a new run

#### Scenario: Released
- **WHEN** the control plane sends `Release` while the runtime waits
- **THEN** the run finalizes with `END_TURN`

#### Scenario: Budget ends while idle
- **WHEN** the wall-clock budget runs out during the wait
- **THEN** the run finalizes with `END_TURN`, not `LIMIT_REACHED`

#### Scenario: Stream lost while idle
- **WHEN** the control plane stream drops during the wait
- **THEN** the runtime keeps waiting and reconnects as usual; the wait ends only on `Prompt`, `Cancel`, `Release` or the budget

### Requirement: Awaiting input after a blocking request
If a turn received a tool result with `await_input_after_turn` and no prompts are queued after it, the runtime SHALL wait as in "Idle wait after a normal turn end" (`iter1-keep-alive-and-review`); a `Release` then finalizes with `BLOCKED`. A prompt SHALL start the next turn (reported as `RUN_PHASE_RUNNING`); `Cancel` SHALL finalize with `CANCELLED`.

#### Scenario: Answer within the window
- **WHEN** the run awaits input and a prompt arrives after 2 minutes with the default 10-minute deadline
- **THEN** the runtime reports `RUN_PHASE_RUNNING` and starts a turn with that prompt

#### Scenario: Blocking request and completion in one turn
- **WHEN** a turn has both a blocking request result and `end_run_after_turn`
- **THEN** the run finalizes with `COMPLETED` without waiting

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
