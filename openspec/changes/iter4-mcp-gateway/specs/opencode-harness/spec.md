## ADDED Requirements

### Requirement: Gateway MCP server for OpenCode
An `opencode` run SHALL get the gateway MCP endpoint through ACP `session/new` and `session/load` like every ACP harness. The generated configuration SHALL NOT define a server named `ext`, and a server `ext` from the repository's configuration SHALL NOT replace the gateway's.

#### Scenario: OpenCode lists the gateway tools
- **WHEN** an `opencode` run of project `DF` with `context7` enabled sends a model request
- **THEN** its tool list contains the pinned `context7` tools from the `ext` server and the factory tools

#### Scenario: Repository defines `ext`
- **WHEN** the repository's `opencode.json` defines a remote MCP server `ext` with another URL
- **THEN** calls to `ext` tools still go to the gateway MCP endpoint
