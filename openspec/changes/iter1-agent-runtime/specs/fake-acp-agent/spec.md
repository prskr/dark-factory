# Spec Delta

## Purpose

A scripted, deterministic ACP agent so the runtime and the whole system can be tested end to end without Claude and without consuming the subscription (ADR-0024).

## ADDED Requirements

### Requirement: Scripted turns
`fake-acp-agent --script <file>` SHALL act as an ACP agent that, for each prompt turn, executes the next scripted turn's steps in order: stream agent text, stream thoughts, emit a tool call with updates, run a shell command in the session cwd, call a tool on a configured MCP server, emit a plan, report usage, and end with a given stop reason.

#### Scenario: Commit in a turn
- **WHEN** a turn's steps are `shell: "echo x > a.txt && git add a.txt && git commit -m add"` then `end: end_turn`
- **THEN** a commit exists in the workspace and the prompt response has stop reason `end_turn`

#### Scenario: Factory tool call
- **WHEN** a step is `mcp: { server: factory, tool: report_progress, args: { summary: "tests pass" } }`
- **THEN** the agent calls the tool through the MCP server from `session/new` and emits a tool call update with the result

### Requirement: Failure modes
Scripts SHALL be able to simulate a provider rate limit (with retry time), a crash, and a turn that hangs until cancelled.

#### Scenario: Rate limit
- **WHEN** a turn ends with `end: { rate_limited: { retry_after_seconds: 3600 } }`
- **THEN** the agent signals the rate limit the same way the spike recorded for the Claude adapter

### Requirement: Session load
The fake agent SHALL persist session ids under `$HOME` and support `session/load` for ids it created; scripts SHALL be able to select turns by run (first session vs loaded session).

#### Scenario: Second run resumes
- **WHEN** run 2 calls `session/load` with the id from run 1
- **THEN** loading succeeds and the script's turns for the loaded session are used
