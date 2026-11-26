## Purpose

Instance-wide registration of third-party MCP servers (an upstream HTTP MCP server plus the one credential Dark Factory uses for it), which projects' runs may use them, and the record of their calls.

## ADDED Requirements

### Requirement: MCP server registration
Users with the roles `board-member` and `admin` (admins) SHALL list, create, edit and delete MCP servers with a name, an upstream URL (MCP Streamable HTTP), a credential reference, a credential scheme and a "Private network" switch (default off). Changes SHALL be audited (`mcp_server.create`, `mcp_server.update`, `mcp_server.delete`).

#### Scenario: Register Context7
- **WHEN** an admin creates server `context7` with URL `https://mcp.context7.com/mcp`, credential reference `context7-key` and scheme `header` with name `CONTEXT7_API_KEY`
- **THEN** the server is stored with "Private network" off, and an audit entry `mcp_server.create` names it

#### Scenario: Register an internal server
- **WHEN** an admin creates server `wiki` with URL `https://mcp.wiki.corp.internal/mcp` and switches "Private network" on
- **THEN** the server is stored, and the gateway connects to it although its name resolves to a private address

#### Scenario: Board member without admin
- **WHEN** a signed-in board member without the `admin` role opens the MCP servers page or posts its form
- **THEN** the request is refused with 403 "Your account lacks the role `admin`."

### Requirement: Server name
A server name SHALL be 1 to 20 characters of lowercase letters, digits and hyphens, start with a letter, end with a letter or digit, contain no two hyphens in a row, and be unique in the instance. The name SHALL NOT change after creation, because it is part of every tool name agents see.

#### Scenario: Invalid name
- **WHEN** an admin creates a server named `my--tools` or `tools-`
- **THEN** the form rejects it and names the allowed characters

#### Scenario: Rename
- **WHEN** an admin edits server `context7`
- **THEN** the form shows the name read-only

### Requirement: Upstream URL
The upstream URL SHALL be an absolute `https` URL without user information or fragment; it MAY have path and query parameters, which are stored and shown as entered. An `http` URL SHALL be accepted only while the instance setting `network.allowPlainHttp` is on. The form SHALL state that the URL is not secret and that a token belongs into the credential reference.

#### Scenario: Plain HTTP URL
- **WHEN** `network.allowPlainHttp` is off and an admin enters `http://mcp.internal/mcp`
- **THEN** the form rejects it

#### Scenario: User information in the URL
- **WHEN** an admin enters `https://user:secret@mcp.example.com/mcp`
- **THEN** the form rejects it and nothing is stored

#### Scenario: Query parameters in the URL
- **WHEN** an admin enters `https://design.example.com/mcp/stream?file=abc&mode=read`
- **THEN** the URL is stored with its query, and the gateway sends both parameters to the upstream

### Requirement: Credential schemes
An MCP server SHALL have one credential scheme: `bearer` (`Authorization: Bearer <value>`), `header` (a header name and optional prefix, `<name>: <prefix><value>`) or `query` (a query parameter name, `?<name>=<value>` added to the URL). Header names of hop-by-hop headers, `Host`, `Content-Length`, `Content-Type`, `Accept`, `Mcp-Session-Id` and `Mcp-Protocol-Version` SHALL be rejected, and so SHALL a query parameter name the URL already contains, compared after percent-decoding.

#### Scenario: Reserved header name
- **WHEN** an admin chooses scheme `header` with name `Mcp-Session-Id`
- **THEN** the form rejects it

#### Scenario: Token as query parameter
- **WHEN** an admin registers `penpot` with URL `https://design.example.com/mcp/stream?file=abc`, scheme `query` with name `token` and credential reference `penpot-jwt`
- **THEN** the gateway sends every upstream request to `https://design.example.com/mcp/stream?file=abc&token=<value>`, with the value URL-encoded

#### Scenario: Parameter already in the URL
- **WHEN** an admin chooses scheme `query` with name `token` for the URL `https://design.example.com/mcp/stream?tok%65n=abc`
- **THEN** the form rejects it, because `tok%65n` decodes to `token`

### Requirement: Credentials stay write-only
An MCP server SHALL reference an existing credential reference (Secret name and key) and SHALL never hold or display a secret value. The credential SHALL never be sent to a sandbox. A credential reference used by a server SHALL NOT be deletable.

#### Scenario: Server page
- **WHEN** an admin opens server `context7`
- **THEN** the page shows the credential reference's name, Secret and key but no value

#### Scenario: Delete a used credential reference
- **WHEN** an admin deletes credential reference `context7-key` while `context7` uses it
- **THEN** the deletion is refused with a message naming `context7`

### Requirement: No factory credentials for MCP servers
An MCP server SHALL NOT use a credential reference that a project uses as its forge credential or that a model provider uses, and a project or model provider SHALL NOT be saved with a credential reference an MCP server uses. The refusal SHALL name the other user of the reference.

#### Scenario: Forge token selected
- **WHEN** an admin saves server `forgejo` with credential reference `forge-bot`, which project `DF` uses as its forge credential
- **THEN** the form rejects it with "credential `forge-bot` is the forge credential of project DF" and nothing is stored

#### Scenario: Project takes an MCP credential
- **WHEN** an admin sets project `DF`'s forge credential to `context7-key`, which server `context7` uses
- **THEN** the form rejects it naming `context7`

### Requirement: Editing a server
Every edit of a server SHALL reach the gateways within about a second and SHALL end their open upstream sessions of that server, so the next call uses the new URL, credential and switch.

#### Scenario: Rotated credential reference
- **WHEN** an admin points `context7` at another credential reference
- **THEN** the next call opens a new upstream session with the new credential

### Requirement: Write tools warning
The server form SHALL state that agents, in tasks and chats, get every tool the server offers with its unreviewed description, including tools that write; that those writes bypass the Git proxy's push checks and the factory's approvals; that the credential's scope is the effective permission; and that a forge token with write access must not be used.

#### Scenario: Form text
- **WHEN** an admin opens the form to create a server
- **THEN** the form shows the warning, including the forge token rule, next to the credential reference

### Requirement: Enablement per project
Admins SHALL enable and disable MCP servers per project on the project settings page ("MCP servers" tab, shown to admins only, like "Integrations"). No server SHALL be enabled for a project by default. Changes SHALL be audited (`project.mcp_servers_update`).

#### Scenario: Board member without admin opens the project settings
- **WHEN** a board member without the `admin` role opens the settings of project `DF`
- **THEN** the page has no "MCP servers" tab, and a direct request to load or save the tab's content is refused with 403

#### Scenario: Enable for a project
- **WHEN** an admin enables `context7` for project `DF`
- **THEN** the next run of `DF` lists the tools of `context7` with the prefix `context7__`

### Requirement: Servers of a run
A run SHALL get every server currently enabled for its project, chats included. Enabling, disabling, editing or deleting a server SHALL apply to calls and tool lists of running runs within about a second. A harness session sees an added server only if it lists tools again; a run started without any enabled server (no `ext`) gets it from its next run.

#### Scenario: Server disabled during a run
- **WHEN** an admin disables `context7` for `DF` while a `DF` run is active
- **THEN** the run's next call to a `context7__` tool is rejected

#### Scenario: Server enabled during a run
- **WHEN** an admin enables `wiki` for `DF` while a `DF` run with `ext` is active
- **THEN** a call to a `wiki__` tool is permitted and a `tools/list` served after the change contains the `wiki__` tools

#### Scenario: First server enabled during a run
- **WHEN** an admin enables `context7` for a project whose active run started without enabled servers
- **THEN** that run has no `ext` server, and the next run of the project does

### Requirement: Servers in use cannot be deleted
Deleting an MCP server that is enabled for a project SHALL be refused, naming the projects.

#### Scenario: Delete a used server
- **WHEN** an admin deletes `context7` while project `DF` has it enabled
- **THEN** the deletion is refused with a message naming `DF`

### Requirement: Call records in the run view
The control plane SHALL store the call records the gateway reports with their run and show them in the run view: time, server, tool (`tools/list` for a failed list), outcome, duration, request and response size. Arguments and results SHALL NOT be stored. Records SHALL be deleted with their run.

#### Scenario: Server missing from the tool list
- **WHEN** `wiki` timed out while the run listed its tools
- **THEN** the run view shows a `wiki` record with tool `tools/list` and outcome `timeout`

#### Scenario: Run with two calls
- **WHEN** a run called `context7__resolve-library-id` once successfully and `wiki__search` once while `wiki` was disabled
- **THEN** the run view lists both, the second with outcome "rejected"
