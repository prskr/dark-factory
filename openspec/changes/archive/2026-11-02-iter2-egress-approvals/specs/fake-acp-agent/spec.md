# fake-acp-agent (delta)

## MODIFIED Requirements

### Requirement: Scripted turns
`fake-acp-agent --script <file>` SHALL act as an ACP agent that, for each prompt turn, executes the next scripted turn's steps in order: stream agent text, stream thoughts, emit a tool call with updates, run a shell command in the session cwd, call a tool on a configured MCP server, fetch a URL through the sandbox's proxy settings, emit a plan, report usage, and end with a given stop reason. A fetch step retries about once a second until it succeeds or its timeout (default 60 s) passes, and streams "Fetched <url>." or "Could not fetch <url>: <error>".

#### Scenario: Commit in a turn
- **WHEN** a turn's steps are `shell: "echo x > a.txt && git add a.txt && git commit -m add"` then `end: end_turn`
- **THEN** a commit exists in the workspace and the prompt response has stop reason `end_turn`

#### Scenario: Factory tool call
- **WHEN** a step is `mcp: { server: factory, tool: report_progress, args: { summary: "tests pass" } }`
- **THEN** the agent calls the tool through the MCP server from `session/new` and emits a tool call update with the result

#### Scenario: Fetch retried until allowed
- **WHEN** a step is `fetch: { url: "https://pypi.org/" }` and the proxy denies the first attempts, then allows the host
- **THEN** the agent retries in the same turn and streams "Fetched https://pypi.org/." once an attempt succeeds
