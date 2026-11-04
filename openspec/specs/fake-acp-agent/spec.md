# fake-acp-agent Specification

## Purpose

A scripted, deterministic ACP agent so the runtime and the whole system can be tested end to end without Claude and without consuming the subscription (ADR-0024).

## Requirements

### Requirement: Scripted turns
`fake-acp-agent --script <file>` SHALL act as an ACP agent that, for each prompt turn, executes the next scripted turn's steps in order: stream agent text, stream thoughts, emit a tool call with updates, run a shell command in the session cwd, call a tool on a configured MCP server, fetch a URL through the sandbox's proxy settings, emit a plan, call the model, and end with a given stop reason.

#### Scenario: Commit in a turn
- **WHEN** a turn's steps are `shell: "echo x > a.txt && git add a.txt && git commit -m add"` then `end: end_turn`
- **THEN** a commit exists in the workspace and the prompt response has stop reason `end_turn`

#### Scenario: Factory tool call
- **WHEN** a step is `mcp: { server: factory, tool: report_progress, args: { summary: "tests pass" } }`
- **THEN** the agent calls the tool through the MCP server from `session/new` and emits a tool call update with the result

### Requirement: Fetch step
A fetch step SHALL retry about once a second until it succeeds or its timeout (default 60 s) passes, and stream "Fetched <url>." or "Could not fetch <url>: <error>".

#### Scenario: Fetch retried until allowed
- **WHEN** a step is `fetch: { url: "https://pypi.org/" }` and the proxy denies the first attempts, then allows the host
- **THEN** the agent retries in the same turn and streams "Fetched https://pypi.org/." once an attempt succeeds

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

### Requirement: Model call step
A `model` step SHALL send one streamed Anthropic Messages request to `$ANTHROPIC_BASE_URL/v1/messages` with the key from `ANTHROPIC_API_KEY` or `CLAUDE_CODE_OAUTH_TOKEN`, trusting the CA from `NODE_EXTRA_CA_CERTS`, and send the received text as agent text in one chunk. A failed request SHALL be streamed as "Model call failed: <status or error>".

#### Scenario: Model call through the gateway
- **WHEN** a step is `model: { prompt: "hi" }` and the gateway forwards to a fake upstream answering "hello"
- **THEN** the agent sends "hello" and the gateway reports one usage record for the run

### Requirement: Sandbox environment in the model step
After the model answer the `model` step SHALL send "Model key in the sandbox: <key>" and "Sandbox environment:" followed by the process environment, one `NAME=value` per line, sorted, so tests can check that no credential reaches the sandbox.

#### Scenario: Placeholder key visible to tests
- **WHEN** a `model` step runs in a sandbox of a run with a model provider
- **THEN** the transcript shows "Model key in the sandbox: df-placeholder" and the environment dump contains no provider credential
