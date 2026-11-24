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
A server name SHALL be 1 to 20 characters of lowercase letters, digits and hyphens, start with a letter, end with a letter or digit, contain no two hyphens in a row, and be unique in the instance. The name SHALL NOT change after creation, because it is part of every tool name agents see.

#### Scenario: Invalid name
- **WHEN** a board member creates a server named `my--tools` or `tools-`
- **THEN** the form rejects it and names the allowed characters

#### Scenario: Rename
- **WHEN** a board member edits server `context7`
- **THEN** the form shows the name read-only

### Requirement: Upstream URL
The upstream URL SHALL be an absolute `https` URL without user information, query or fragment. An `http` URL SHALL be accepted only while the instance setting `network.allowPlainHttp` is on.

#### Scenario: Plain HTTP URL
- **WHEN** `network.allowPlainHttp` is off and a board member enters `http://mcp.internal/mcp`
- **THEN** the form rejects it

#### Scenario: Credentials in the URL
- **WHEN** a board member enters `https://user:secret@mcp.example.com/mcp` or `https://mcp.example.com/mcp?key=secret`
- **THEN** the form rejects it and nothing is stored

### Requirement: Editing a server
Changing a server's URL SHALL unselect all its tools and discard its stored fetch, so that tools reviewed for one upstream are never served for another. Changing the credential or scheme SHALL keep the selection. Every edit SHALL end the gateways' open upstream sessions of that server.

#### Scenario: New URL
- **WHEN** a board member changes the URL of `context7`, which has two pinned tools
- **THEN** after saving the server has no pinned tools, and runs of projects that enable it list no `context7__` tools

#### Scenario: Rotated credential reference
- **WHEN** a board member points `context7` at another credential reference
- **THEN** the pinned tools stay, and the next call opens a new upstream session with the new credential

### Requirement: Credential schemes
An MCP server SHALL have one credential scheme: `bearer` (`Authorization: Bearer <value>`) or `header` (a header name and optional prefix, `<name>: <prefix><value>`). Header names of hop-by-hop headers, `Host`, `Content-Length`, `Content-Type`, `Accept`, `Mcp-Session-Id` and `Mcp-Protocol-Version` SHALL be rejected.

#### Scenario: Reserved header name
- **WHEN** a board member chooses scheme `header` with name `Mcp-Session-Id`
- **THEN** the form rejects it

### Requirement: Credentials stay write-only
An MCP server SHALL reference an existing credential reference (Secret name and key) and SHALL never hold or display a secret value. The credential SHALL never be sent to a sandbox. A credential reference used by a server SHALL NOT be deletable.

#### Scenario: Server page
- **WHEN** a board member opens server `context7`
- **THEN** the page shows the credential reference's name, Secret and key but no value

#### Scenario: Delete a used credential reference
- **WHEN** a board member deletes credential reference `context7-key` while `context7` uses it
- **THEN** the deletion is refused with a message naming `context7`

### Requirement: Fetch tools
On "Fetch tools" the control plane SHALL connect to the upstream with the server's credential, list its tools (`tools/list`, all pages, at most 1,000 tools and 4 MiB) and show each tool's name, title, description, input and output schema and annotations. It SHALL apply the gateway's upstream destination rules, and show an error naming the cause without any credential when the fetch fails.

#### Scenario: Fetch succeeds
- **WHEN** a board member fetches the tools of `context7` and the upstream lists `resolve-library-id` and `get-library-docs`
- **THEN** the page shows both tools with their full descriptions and schemas, both unselected

#### Scenario: Private address
- **WHEN** the server's host resolves to `10.0.4.7`
- **THEN** the fetch fails with "destination is a private address" and no connection is opened

#### Scenario: Redirect
- **WHEN** the upstream answers with a 3xx redirect
- **THEN** the redirect is not followed and the fetch fails with "upstream redirected"

#### Scenario: Wrong credential
- **WHEN** the upstream answers the fetch with 401
- **THEN** the page shows "upstream refused the credential (401)" and the stored selection is unchanged

#### Scenario: Secret missing
- **WHEN** the server's Secret or key does not exist
- **THEN** the fetch fails with an error naming Secret and key, and no upstream is contacted

#### Scenario: Not an MCP server or too large
- **WHEN** the upstream answers with HTML, does not answer within 30 s, or lists more than 1,000 tools or 4 MiB
- **THEN** the fetch fails with "not an MCP server", "upstream timeout" or "tool list too large", and the stored selection is unchanged

### Requirement: Pinned tool selection
A board member SHALL select which fetched tools agents may use; no tool SHALL be selected by default. Saving SHALL store the selected tools with exactly the definitions shown in that fetch (pinned). A save SHALL be refused when another fetch of the server happened after the one shown, or when it names a tool that fetch did not list. Saving SHALL be audited with the selected tool names.

#### Scenario: Select two tools
- **WHEN** a board member selects `resolve-library-id` and `get-library-docs` and saves
- **THEN** both are pinned with the descriptions and schemas shown, and an audit entry `mcp_server.tools_update` lists them

#### Scenario: Stale fetch
- **WHEN** board member A has a fetch open, board member B fetches the same server, and A saves
- **THEN** A's save is refused with "the tool list was fetched again, review it before saving" and nothing changes

#### Scenario: Unknown tool name
- **WHEN** a save names `delete_repo`, which the shown fetch did not list
- **THEN** the save is refused and nothing changes

#### Scenario: Upstream changes a description
- **WHEN** the upstream changes the description of the pinned tool `get-library-docs` after it was saved
- **THEN** runs keep seeing the pinned description until a board member fetches and saves again

### Requirement: Pinned definitions are bounded
A server's pinned definitions SHALL be at most 100 tools and 256 KiB, and all pinned definitions of the instance together at most 2 MiB. A save over a limit SHALL be refused, naming the limit.

#### Scenario: Too many bytes
- **WHEN** a board member selects tools whose definitions total 300 KiB
- **THEN** the save is refused with "pinned tools exceed 256 KiB for this server" and nothing changes

### Requirement: Annotations shown, not trusted
The selection SHALL show each tool's `readOnlyHint` and `destructiveHint` annotations and SHALL mark every tool not annotated `readOnlyHint: true` as "may write", with the warning that its writes bypass the Git proxy's push checks and the factory's approvals. Annotations SHALL NOT select or enable a tool by themselves.

#### Scenario: Write tool
- **WHEN** the fetched tools include `create_issue` without annotations
- **THEN** it is shown unselected with the mark "may write" and the warning

### Requirement: Tool names agents can use
A tool SHALL be selectable only if its name matches `^[A-Za-z0-9_-]{1,64}$` and the name the harness sees, `mcp__ext__<server>__<tool>`, has at most 64 characters. Other tools SHALL be shown disabled with the reason.

#### Scenario: Name too long
- **WHEN** server `forgejo` lists a tool whose name has 50 characters
- **THEN** the tool is shown disabled with "name too long for harnesses (max. 64 with prefix)"

### Requirement: Refetch marks changes
A fetch for a server with pinned tools SHALL mark each fetched tool as new, changed (title, description, input schema, output schema or annotations differ from the pinned definition) or unchanged, and list pinned tools the upstream no longer offers as removed. New and changed tools SHALL be shown unselected, so a changed tool stays pinned only if the board member selects it again. Nothing SHALL change for runs until the board member saves.

#### Scenario: New tool upstream
- **WHEN** a board member fetches again and the upstream now also lists `delete_repo`
- **THEN** `delete_repo` is shown as new and unselected, and runs see no change

#### Scenario: Changed and removed tools
- **WHEN** a fetch shows pinned `get-library-docs` changed and pinned `resolve-library-id` removed, and the board member saves without selecting anything
- **THEN** neither tool is pinned any more

#### Scenario: Changed tool accepted
- **WHEN** a fetch shows pinned `get-library-docs` changed, and the board member selects it and saves
- **THEN** `get-library-docs` is pinned with its new definition

### Requirement: Enablement per project
Board members SHALL enable and disable MCP servers per project on the project settings page ("MCP servers" tab). No server SHALL be enabled for a project by default. Changes SHALL be audited (`project.mcp_servers_update`). A server with no pinned tools SHALL be shown, but enabling it has no effect on runs until tools are pinned.

#### Scenario: Enable for a project
- **WHEN** a board member enables `context7` for project `DF`
- **THEN** the next run of `DF` lists `context7__resolve-library-id` and `context7__get-library-docs`

### Requirement: Tool set of a run
A run's tool set SHALL be the pinned tools of every server enabled for the run's project, chats included. Removals (a server disabled, edited or deleted, a tool unpinned) SHALL apply to calls of running runs within about a second. Additions SHALL be listed from the next run on.

#### Scenario: Server disabled during a run
- **WHEN** a board member disables `context7` for `DF` while a `DF` run is active
- **THEN** the run's next call to a `context7__` tool is rejected

#### Scenario: Server enabled during a run
- **WHEN** a board member enables `context7` for `DF` while a `DF` run is active
- **THEN** the running session lists no `context7__` tools, and the next run of `DF` does

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
