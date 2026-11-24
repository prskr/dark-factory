## Purpose

Instance-wide registration of third-party MCP servers (an upstream HTTP MCP server plus the one credential Dark Factory uses for it), the pinned list of tools agents may use, and which projects' runs get them.

## ADDED Requirements

### Requirement: MCP server registration
Board members SHALL list, create, edit and delete MCP servers with a name, an upstream URL (MCP Streamable HTTP), a credential reference and a credential scheme. Changes SHALL be audited (`mcp_server.create`, `mcp_server.update`, `mcp_server.delete`).

#### Scenario: Register Context7
- **WHEN** a board member creates server `context7` with URL `https://mcp.context7.com/mcp`, credential reference `context7-key` and scheme `header` with name `CONTEXT7_API_KEY`
- **THEN** the server is stored with no tools selected, and an audit entry `mcp_server.create` names it

#### Scenario: Non-board member
- **WHEN** a signed-in user without the `board-member` role opens the MCP servers page or posts its form
- **THEN** the request is refused with 403

### Requirement: Server name
A server name SHALL be 1 to 20 characters of lowercase letters, digits and single hyphens, start with a letter, and be unique in the instance. The name SHALL NOT change after creation, because it is part of every tool name agents see.

#### Scenario: Name with underscores
- **WHEN** a board member creates a server named `my__tools`
- **THEN** the form rejects it and names the allowed characters

#### Scenario: Rename
- **WHEN** a board member edits server `context7`
- **THEN** the form shows the name read-only

### Requirement: Upstream URL
The upstream URL SHALL be an absolute `https` URL without user information or fragment. An `http` URL SHALL be accepted only while the instance setting `network.allowPlainHttp` is on.

#### Scenario: Plain HTTP URL
- **WHEN** `network.allowPlainHttp` is off and a board member enters `http://mcp.internal/mcp`
- **THEN** the form rejects it

#### Scenario: Credentials in the URL
- **WHEN** a board member enters `https://user:secret@mcp.example.com/mcp`
- **THEN** the form rejects it and nothing is stored

### Requirement: Credential schemes
An MCP server SHALL have one credential scheme: `bearer` (`Authorization: Bearer <value>`) or `header` (a header name and optional prefix, `<name>: <prefix><value>`). Header names of hop-by-hop headers, `Host`, `Content-Length`, `Content-Type`, `Accept`, `Mcp-Session-Id` and `Mcp-Protocol-Version` SHALL be rejected.

#### Scenario: Reserved header name
- **WHEN** a board member chooses scheme `header` with name `Mcp-Session-Id`
- **THEN** the form rejects it

### Requirement: Credentials stay write-only
An MCP server SHALL reference an existing credential reference (Secret name and key) and SHALL never hold or display a secret value. The credential SHALL never be sent to a sandbox.

#### Scenario: Server page
- **WHEN** a board member opens server `context7`
- **THEN** the page shows the credential reference's name, Secret and key but no value

### Requirement: Fetch tools
On "Fetch tools" the control plane SHALL connect to the upstream with the server's credential, list its tools (`tools/list`, all pages) and show each tool's name, title, description, input schema and annotations. It SHALL apply the gateway's upstream destination rules and limits, and show an error naming the cause without any credential when the fetch fails.

#### Scenario: Fetch succeeds
- **WHEN** a board member fetches the tools of `context7` and the upstream lists `resolve-library-id` and `get-library-docs`
- **THEN** the page shows both tools with their full descriptions and schemas, both unselected

#### Scenario: Private address
- **WHEN** the server's host resolves to `10.0.4.7`
- **THEN** the fetch fails with "destination is a private address" and no connection is opened

#### Scenario: Wrong credential
- **WHEN** the upstream answers the fetch with 401
- **THEN** the page shows "upstream refused the credential (401)" and the stored selection is unchanged

### Requirement: Pinned tool selection
A board member SHALL select which fetched tools agents may use; no tool SHALL be selected by default. Saving SHALL store the selected tools with their definitions as fetched (pinned). Agents SHALL see only pinned definitions, never the upstream's current ones. Saving SHALL be audited with the selected tool names.

#### Scenario: Select two tools
- **WHEN** a board member selects `resolve-library-id` and `get-library-docs` and saves
- **THEN** both are pinned with the descriptions and schemas shown, and an audit entry `mcp_server.tools_update` lists them

#### Scenario: Upstream changes a description
- **WHEN** the upstream changes the description of the pinned tool `get-library-docs` after it was saved
- **THEN** runs keep seeing the pinned description until a board member fetches and saves again

### Requirement: Annotations shown, not trusted
The selection SHALL show each tool's `readOnlyHint` and `destructiveHint` annotations and SHALL mark every tool not annotated `readOnlyHint: true` as "may write". Annotations SHALL NOT select or enable a tool by themselves.

#### Scenario: Write tool
- **WHEN** the fetched tools include `create_issue` without annotations
- **THEN** it is shown unselected with the mark "may write"

### Requirement: Tool names agents can use
A tool SHALL be selectable only if its name matches `^[A-Za-z0-9_-]{1,64}$` and the name the harness sees, `mcp__ext__<server>__<tool>`, has at most 64 characters. Other tools SHALL be shown disabled with the reason.

#### Scenario: Name too long
- **WHEN** server `forgejo` lists a tool whose name has 50 characters
- **THEN** the tool is shown disabled with "name too long for harnesses (max. 64 with prefix)"

### Requirement: Refetch marks changes
A fetch for a server with pinned tools SHALL mark each fetched tool as new, changed (description, title, input schema or annotations differ from the pinned definition) or unchanged, and list pinned tools the upstream no longer offers as removed. New tools SHALL be unselected. Nothing SHALL change for runs until the board member saves.

#### Scenario: New tool upstream
- **WHEN** a board member fetches again and the upstream now also lists `delete_repo`
- **THEN** `delete_repo` is shown as new and unselected, and runs see no change

#### Scenario: Changed and removed tools
- **WHEN** a fetch shows pinned `get-library-docs` changed and pinned `resolve-library-id` removed, and the board member saves
- **THEN** `get-library-docs` is pinned with its new definition and `resolve-library-id` is no longer pinned

### Requirement: Enablement per project
Board members SHALL enable and disable MCP servers per project on the project settings page ("MCP servers" tab). Changes SHALL be audited (`project.mcp_servers_update`). A server with no pinned tools SHALL be shown, but enabling it has no effect on runs until tools are pinned.

#### Scenario: Enable for a project
- **WHEN** a board member enables `context7` for project `DF`
- **THEN** the next `tools/list` of a `DF` run lists `context7__resolve-library-id` and `context7__get-library-docs`

### Requirement: Tool set of a run
A run's tool set SHALL be the pinned tools of every server enabled for the run's project, chats included. Changes to the pinned tools or the enablement SHALL reach running runs within about a second.

#### Scenario: Server disabled during a run
- **WHEN** a board member disables `context7` for `DF` while a `DF` run is active
- **THEN** the run's next call to a `context7__` tool is rejected

### Requirement: Servers in use cannot be deleted
Deleting an MCP server that is enabled for a project SHALL be refused, naming the projects.

#### Scenario: Delete a used server
- **WHEN** a board member deletes `context7` while project `DF` has it enabled
- **THEN** the deletion is refused with a message naming `DF`

### Requirement: Call records in the run view
The control plane SHALL store the call records the gateway reports with their run and show them in the run view: time, server, tool, outcome, duration, request and response size. Arguments and results SHALL NOT be stored. Records SHALL be deleted with their run.

#### Scenario: Run with two calls
- **WHEN** a run called `context7__resolve-library-id` once successfully and `context7__delete_repo` once
- **THEN** the run view lists both, the second with outcome "rejected"
