## Purpose

Instance-wide registration of third-party MCP servers (an upstream HTTP MCP server plus the one credential Dark Factory uses for it), which projects' runs may use them, and the record of their calls.

## ADDED Requirements

### Requirement: MCP server registration
Board members SHALL list, create, edit and delete MCP servers with a name, an upstream URL (MCP Streamable HTTP), a credential reference, a credential scheme and a "Private network" switch (default off). Changes SHALL be audited (`mcp_server.create`, `mcp_server.update`, `mcp_server.delete`).

#### Scenario: Register Context7
- **WHEN** a board member creates server `context7` with URL `https://mcp.context7.com/mcp`, credential reference `context7-key` and scheme `header` with name `CONTEXT7_API_KEY`
- **THEN** the server is stored with "Private network" off, and an audit entry `mcp_server.create` names it

#### Scenario: Register an internal server
- **WHEN** a board member creates server `wiki` with URL `https://mcp.wiki.corp.internal/mcp` and switches "Private network" on
- **THEN** the server is stored, and the gateway connects to it although its name resolves to a private address

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

### Requirement: No factory credentials for MCP servers
An MCP server SHALL NOT use a credential reference that a project uses as its forge credential or that a model provider uses, and a project or model provider SHALL NOT be saved with a credential reference an MCP server uses. The refusal SHALL name the other user of the reference.

#### Scenario: Forge token selected
- **WHEN** a board member saves server `forgejo` with credential reference `forge-bot`, which project `DF` uses as its forge credential
- **THEN** the form rejects it with "credential `forge-bot` is the forge credential of project DF" and nothing is stored

#### Scenario: Project takes an MCP credential
- **WHEN** a board member sets project `DF`'s forge credential to `context7-key`, which server `context7` uses
- **THEN** the form rejects it naming `context7`

### Requirement: Editing a server
Every edit of a server SHALL reach the gateways within about a second and SHALL end their open upstream sessions of that server, so the next call uses the new URL, credential and switch.

#### Scenario: Rotated credential reference
- **WHEN** a board member points `context7` at another credential reference
- **THEN** the next call opens a new upstream session with the new credential

### Requirement: Write tools warning
The server form SHALL state that agents, in tasks and chats, get every tool the server offers with its unreviewed description, including tools that write; that those writes bypass the Git proxy's push checks and the factory's approvals; that the credential's scope is the effective permission; and that a forge token with write access must not be used.

#### Scenario: Form text
- **WHEN** a board member opens the form to create a server
- **THEN** the form shows the warning, including the forge token rule, next to the credential reference

### Requirement: Enablement per project
Board members SHALL enable and disable MCP servers per project on the project settings page ("MCP servers" tab). No server SHALL be enabled for a project by default. Changes SHALL be audited (`project.mcp_servers_update`).

#### Scenario: Enable for a project
- **WHEN** a board member enables `context7` for project `DF`
- **THEN** the next run of `DF` lists the tools of `context7` with the prefix `context7__`

### Requirement: Servers of a run
A run SHALL get every server currently enabled for its project, chats included. Enabling, disabling, editing or deleting a server SHALL apply to calls and tool lists of running runs within about a second. A harness session sees an added server only if it lists tools again; a run started without any enabled server (no `ext`) gets it from its next run.

#### Scenario: Server disabled during a run
- **WHEN** a board member disables `context7` for `DF` while a `DF` run is active
- **THEN** the run's next call to a `context7__` tool is rejected

#### Scenario: Server enabled during a run
- **WHEN** a board member enables `wiki` for `DF` while a `DF` run with `ext` is active
- **THEN** a call to a `wiki__` tool is permitted and a `tools/list` served after the change contains the `wiki__` tools

#### Scenario: First server enabled during a run
- **WHEN** a board member enables `context7` for a project whose active run started without enabled servers
- **THEN** that run has no `ext` server, and the next run of the project does

### Requirement: Servers in use cannot be deleted
Deleting an MCP server that is enabled for a project SHALL be refused, naming the projects.

#### Scenario: Delete a used server
- **WHEN** a board member deletes `context7` while project `DF` has it enabled
- **THEN** the deletion is refused with a message naming `DF`

### Requirement: Call records in the run view
The control plane SHALL store the call records the gateway reports with their run and show them in the run view: time, server, tool (`tools/list` for a failed list), outcome, duration, request and response size. Arguments and results SHALL NOT be stored. Records SHALL be deleted with their run.

#### Scenario: Server missing from the tool list
- **WHEN** `wiki` timed out while the run listed its tools
- **THEN** the run view shows a `wiki` record with tool `tools/list` and outcome `timeout`

#### Scenario: Run with two calls
- **WHEN** a run called `context7__resolve-library-id` once successfully and `wiki__search` once while `wiki` was disabled
- **THEN** the run view lists both, the second with outcome "rejected"
