## ADDED Requirements

### Requirement: Gateway MCP endpoint in Assign
For a run whose project has at least one enabled MCP server with pinned tools, `Assign` SHALL carry the URL of the gateway's MCP endpoint (`https://<gateway git host>:<git port>/mcp`). Otherwise the field SHALL be empty. Chat runs SHALL be treated the same way.

#### Scenario: Project with Context7
- **WHEN** a run of project `DF` starts and `DF` has `context7` enabled with pinned tools
- **THEN** `Assign` carries `https://<gateway>:8443/mcp`

#### Scenario: Project without MCP servers
- **WHEN** a run of a project without enabled servers starts
- **THEN** `Assign` carries no MCP endpoint
