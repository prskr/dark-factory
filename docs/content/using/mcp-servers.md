---
title: MCP servers
linkTitle: MCP servers
weight: 6
---

An **MCP server** is a third-party tool server that agents reach through the
[gateway's MCP endpoint](../security/mcp-endpoint.md): documentation search, an internal wiki, a
design tool. You register it once for the whole instance, with the one credential Dark Factory uses
for it, and enable it per project. Agents never hold the credential: the gateway adds it on the way
out. Registering and enabling need the role `admin`.

Dark Factory forwards every tool of an enabled server, with the description the upstream sends now,
and the credential's scope is the real permission. Read the
[security page](../security/mcp-endpoint.md#what-this-does-not-protect) first: write tools bypass
the Git proxy's push checks and the factory's approvals.

## Register a server

**Settings → MCP servers → New MCP server**. Every change is audited (`mcp_server.create`,
`mcp_server.update`, `mcp_server.delete`).

| Field | Meaning |
| --- | --- |
| Name | 1 to 20 lowercase letters, digits and single hyphens, starting with a letter and ending with a letter or digit; unique. Agents see the tools as `<name>__<tool>`, so the name cannot change after creation. |
| URL | The server's MCP Streamable HTTP endpoint, `https` (`http` only with `network.allowPlainHttp`), without user information or fragment. Path and query parameters are kept. **The URL is not secret**: it is stored and shown. A token belongs into the credential reference. |
| Credential reference | The [credential reference](setup.md#credential-references) that names the Secret key with the server's token. It must not be a project's forge credential or a model provider's credential, and a project or provider cannot take one an MCP server uses. A credential reference that a server uses cannot be deleted. |
| Credential scheme | `Bearer` (`Authorization: Bearer <value>`), `Header` (a header name and an optional prefix, `<name>: <prefix><value>`) or `Query parameter` (`?<name>=<value>` after the URL's own parameters, URL-encoded). The form previews what is sent. Hop-by-hop headers, `Host`, `Content-Length`, `Content-Type`, `Accept`, `Mcp-Session-Id` and `Mcp-Protocol-Version` are refused as header names, and so is a query parameter name the URL already contains (compared after percent-decoding). |
| Private network | Off by default. On for servers on an internal address you chose on purpose: it skips the check that refuses private, link-local and loopback addresses. Use it only for names in DNS you control; it also allows `localhost` and the gateway's own ports. See [the destination check](../security/mcp-endpoint.md#private-network-and-the-destination-check). |

An edit reaches the gateways within about a second and ends their open sessions with the server, so
the next call uses the new URL, credential and switch.

### Example: Context7

Context7 documents `Authorization: Bearer <key>` (keys start with `ctx7sk`) and also accepts a
`CONTEXT7_API_KEY` header. It works without a key at lower rate limits; an invalid key does not
fail the request, it returns a tool result with `isError: true`.

1. Create a [credential reference](setup.md#credential-references) `context7-key` for the Secret key
   that holds the API key.
2. Register `context7` with URL `https://mcp.context7.com/mcp`, credential reference `context7-key`
   and scheme **Bearer**.
3. Enable it for a project (below). The tools `context7__resolve-library-id` and
   `context7__query-docs` appear in the project's next run.

### Example: a server that takes the token as a query parameter

PenPot's MCP server takes its MCP key as the query parameter `userToken`. Register `penpot` with URL
`https://<penpot-domain>/mcp/stream`, credential reference for the MCP key, scheme **Query
parameter** and name `userToken`. The gateway sends every request to
`https://<penpot-domain>/mcp/stream?userToken=<value>`. The upstream and proxies in front of it may
log full URLs, so use a header scheme where the server supports one.

### Example: an internal server

Register `wiki` with URL `https://mcp.wiki.corp.internal/mcp`, a credential reference and the scheme
the server expects, and switch **Private network** on. Without the switch the gateway refuses a
name that resolves to a private address.

## Enable a server for a project

On the project's settings page, the tab **MCP servers** (after **Integrations**, admins only) lists
every registered server with a checkbox. **Save MCP servers** saves the tab on its own (the project's
**Save project** is not used); the change is audited as `project.mcp_servers_update`. Nothing is
enabled by default. Runs of the project, chats included, get every tool of the enabled servers.

Changes apply to running runs at once: a server disabled during a run is rejected on the next call
and missing from the next tool list; a server enabled during a run is callable at once, but the
harness sees its tools only when it lists again, which most harnesses do once per session. A run
that started without any enabled server has no MCP relay and gets one from its next run. Enabling a
server for a project gives that project's agents everything the credential reaches. A server that is
enabled for a project cannot be deleted; the list says for which projects.

## Call records

The run view shows a section **Tool calls (MCP)** in each run card that has records: the tool
(`<server>__<tool>`), an outcome badge, the time, the duration and the request and response sizes.
Arguments and results are not stored; the transcript has the harness's view of the call.

| Outcome | Meaning |
| --- | --- |
| `ok` | The upstream returned a result |
| `tool-error` | The upstream returned `isError: true` or a JSON-RPC error |
| `rejected` | The server is not enabled for the run, or the run already had 8 calls in progress |
| `upstream-error` | The upstream could not be reached, refused the credential (`401`), answered with an error status or something that is not MCP, redirected, or its address was refused |
| `timeout` | The upstream did not answer in time |
| `too-large` | The result exceeded 4 MiB (or a tool list exceeded its limits) |
| `cancelled` | The client gave up the call |

A server missing from a run's tool list shows as a record `<server> · tools/list` with the reason as
outcome (`upstream-error`, `timeout`, `too-large`), so you can see why an agent lacks tools.
