# Claude Agent ACP Spike Findings & Specification

This document captures the behavior, constraints, and integration specifics of `@zed-industries/claude-agent-acp` (and its updated package `@agentclientprotocol/claude-agent-acp`) when running headless inside the Dark Factory sandbox environment (`crates/factory-agent`).

## 1. Execution & Process Model

- **Invocation**: The adapter is invoked as a headless child process communicating over standard input and standard output using JSON-RPC according to the Agent Client Protocol (ACP).
- **Process Working Directory**: `/data/repo`
- **Environment**:
  - `HOME=/data/home`
  - `CLAUDE_CODE_OAUTH_TOKEN`: Injected into the subprocess environment; never persisted to files under `/data`.
  - `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY`: Inherited from pod environment to route all external network requests through the sandbox egress gateway.
- **Client Capabilities**: `factory-agent` advertises no client filesystem or terminal capabilities to ensure all actions route through designated tools or monitored sandbox operations.

## 2. Session Lifecycle & Continuity

### `session/new`
- Settings are configured by writing `/data/home/.claude/settings.json`.
- The factory stdio MCP server (`/opt/factory/factory-agent mcp`) is passed in `session/new` options as the `factory` server.

### `session/load` & Resumption
- When `Assign.continuation.harness_session_id` is present, `factory-agent` attempts `session/load`.
- If the adapter returns an error or does not support `session/load`, `factory-agent` falls back to `session/new`, prepending `Assign.continuation.summary` to the first prompt, and emits a `harness.session_resume_failed` notice to the control plane.

## 3. Persona System Prompt Injection

- Claude Code allows appending project guidelines via `/data/home/.claude/CLAUDE.md` or through `_meta` attributes on prompt requests.
- Before launching or initiating a prompt turn, `factory-agent` ensures `/data/home/.claude/CLAUDE.md` contains the persona's system prompt from `Assign.persona.system_prompt`.

## 4. Token Usage Reporting & Session Logs

- If the ACP adapter reports token usage in `session/update` or `TurnEnded` events (`USAGE_SOURCE_HARNESS`), those metrics are captured directly.
- **Fallback (`USAGE_SOURCE_SESSION_LOG`)**: When the adapter omits token usage, `factory-agent` scans Claude Code's session log at:
  `/data/home/.claude/projects/<cwd-slug>/<session-id>.jsonl`
  summing `message.usage` (input tokens, output tokens, cache read, cache write) across assistant messages recorded during the turn.

## 5. Rate Limiting & Stop Reasons

- Provider rate limits surface in the adapter as turn errors or stop reason indications containing retry guidance.
- When detected:
  - The runtime maps this to `TurnEnded.stop_reason = RATE_LIMITED`.
  - `retry_after_seconds` is extracted from the error message or header payload if present.
  - The run finalizes with `RUN_OUTCOME_RATE_LIMITED`.

## 6. Network Egress & Egress Hosts

When running behind the sandbox egress proxy with `HTTPS_PROXY`, the adapter contacts:
- `api.anthropic.com` (Claude API)
- `statsig.anthropic.com` / telemetry endpoints (blocked or allowed depending on gateway policy)
- Any external hosts accessed by harness tools if permitted by gateway allowlists.
No direct TCP connections outside the configured proxy are permitted in the sandbox.

## 7. Factory Tool Correlation

- Tools provided by `factory-agent mcp` are registered under server name `factory`.
- Tool calls emitted by Claude ACP are prefixed as `mcp__factory__<tool_name>`.
- The runtime maps these calls to `EventOrigin::FACTORY` and correlates `factory_call_id` with `ToolCallRequest` dispatched to the control plane.

## 8. Manual Verification with Real Claude ACP Adapter

An integration test `test_manual_claude_acp_run` in `crates/factory-agent/src/tests.rs` is provided to manually verify `factory-agent` against the real `@agentclientprotocol/claude-agent-acp` adapter and Anthropic Claude APIs using a subscription token.

### Prerequisites

1. Active Claude Code subscription / OAuth token.
2. Local Node.js (`npx`) environment.
3. Network access to Anthropic API endpoints (`api.anthropic.com`).

### Execution Command

```bash
CLAUDE_CODE_OAUTH_TOKEN="sk-ant-oat01-..." cargo test -p factory-agent -- --ignored test_manual_claude_acp_run --nocapture
```

### Verification Flow

1. **Bootstrap**: Initializes a temporary local git repository with `README.md` and spins up the local git server.
2. **Configuration**: Constructs `Assign` message with MCP tool `complete_task`, persona system prompt, and workspace configuration for task branch `factory/task/DF-1`.
3. **Execution**: Spawns `@agentclientprotocol/claude-agent-acp` via stdio ACP, sets up `factory-agent mcp` Unix domain socket, and sends the prompt: `"Read README.md, append '# Claude Edit' and call complete_task."`.
4. **Tool Dispatch**: Claude reads the file, edits it, and invokes `mcp__factory__complete_task`. The scripted control plane receives the tool request over mTLS and replies with `end_run_after_turn: true`.
5. **Outcome**: The turn ends with `RunOutcome::Completed`. The workspace commits the modified `README.md` and pushes it to the remote task branch.

## 9. Sample Transcript

The following sample transcript illustrates the real-time event stream generated during an execution turn with Claude ACP:

```json
{"turn_started": {"turn_id": "turn-1", "started_at": "2026-10-18T15:30:00.100Z"}}
{"user_message": {"message_id": "msg-prompt-1", "origin": "EVENT_ORIGIN_USER", "content": [{"text": {"text": "Read README.md, append '# Claude Edit' and call complete_task."}}]}}
{"agent_thought_chunk": {"chunk_id": "thought-1", "text": "I will read README.md first to inspect its contents, modify it to append '# Claude Edit', and then call the complete_task tool."}}
{"tool_call": {"call_id": "call-read-file-1", "tool_name": "read_file", "origin": "EVENT_ORIGIN_HARNESS", "input_json": "{\"path\":\"README.md\"}"}}
{"tool_call_update": {"call_id": "call-read-file-1", "status": "COMPLETED", "output_json": "{\"content\":\"# Example Project\\nInitial content.\"}"}}
{"agent_thought_chunk": {"chunk_id": "thought-2", "text": "Now appending '# Claude Edit' to README.md."}}
{"tool_call": {"call_id": "call-write-file-1", "tool_name": "write_file", "origin": "EVENT_ORIGIN_HARNESS", "input_json": "{\"path\":\"README.md\",\"content\":\"# Example Project\\nInitial content.\\n\\n# Claude Edit\\n\"}"}}
{"tool_call_update": {"call_id": "call-write-file-1", "status": "COMPLETED", "output_json": "{\"success\":true}"}}
{"agent_thought_chunk": {"chunk_id": "thought-3", "text": "Task edits complete. Now notifying the control plane via complete_task."}}
{"tool_call": {"call_id": "call-complete-task-1", "tool_name": "mcp__factory__complete_task", "origin": "EVENT_ORIGIN_FACTORY", "factory_call_id": "0192a3f0-4567-789a-bcde-f0123456789a", "input_json": "{}"}}
{"tool_call_update": {"call_id": "call-complete-task-1", "status": "COMPLETED", "output_json": "{}"}}
{"agent_message_chunk": {"chunk_id": "msg-chunk-1", "text": "I have updated README.md and marked the task as complete."}}
{"turn_ended": {
  "turn_id": "turn-1",
  "stop_reason": "STOP_REASON_END_TURN",
  "ended_at": "2026-10-18T15:30:12.450Z",
  "usage": {
    "source": "USAGE_SOURCE_HARNESS",
    "input_tokens": 1420,
    "output_tokens": 315,
    "cache_read_input_tokens": 850,
    "cache_creation_input_tokens": 570
  }
}}
{"run_report": {
  "run_id": "run-claude-manual",
  "outcome": "RUN_OUTCOME_COMPLETED",
  "turns_completed": 1,
  "head_commit": "a1b2c3d4e5f67890123456789abcdef01234567",
  "task_branch": "factory/task/DF-1",
  "pushed": true
}}
```

