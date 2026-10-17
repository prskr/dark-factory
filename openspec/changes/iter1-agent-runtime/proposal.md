# Proposal

## Why

Inside each sandbox, something has to dial back to the control plane, own the Git workspace safely, drive Claude Code over ACP, stream a transcript, proxy the factory tools and push the work. That is the runtime (`factory-agent`), plus a scripted fake ACP agent so all of this can be tested without Claude (milestones 5 and 7 of `product/iterations/iteration-1.md`).

## What Changes

- Spikes: Claude ACP adapter headless with `CLAUDE_CODE_OAUTH_TOKEN`, `session/load`, token usage reporting, system prompt injection, rate-limit signalling, `HTTPS_PROXY` support and contacted hosts.
- `factory-agent run`: mTLS connect with file-reloaded certificates, `Hello`/resume, on-disk outbox with replay, exponential reconnect.
- Workspace prepare/finalize per `design/agent-run.md` §6.2/§6.5: blobless clone, task branch, runtime-owned `.git/config`, hardened Git invocations, push after every turn with new commits, WIP snapshot.
- `factory-agent git-credential`: Git credential helper backed by `CredentialRequest`.
- ACP client: spawn the configured adapter, `session/new` or `session/load` (summary fallback), turn-based prompt delivery, auto-approved permission requests, translation of `session/update` into transcript v1, usage capture, budgets, rate-limit detection, run outcome.
- `factory-agent mcp`: stdio MCP server `factory` bridged over a Unix socket on tmpfs; `tools/list` from `Assign.tools`, calls forwarded as `ToolCallRequest`.
- `fake-acp-agent`: scripted ACP agent (messages, thoughts, tool calls, shell commands that commit, factory MCP calls, plans, usage, stop reasons incl. rate limit, `session/load`).

## Capabilities

### New Capabilities
- `runtime-connection`: connecting, resuming and reliable delivery from the runtime's side.
- `workspace-lifecycle`: how a task's workspace is prepared, pushed and finalized.
- `git-hardening`: what the runtime does to keep Git safe on an untrusted volume.
- `acp-harness`: driving an ACP harness through a run and turning its output into the transcript and a run outcome.
- `factory-mcp-server`: how the harness sees and calls the factory tools.
- `fake-acp-agent`: the scripted test harness.

### Modified Capabilities
None.

## Impact

- New crates `factory-agent` (bin) and `fake-acp-agent` (bin); support files `hooks-empty/` and `gitconfig` for the runtime image.
- Depends only on `df-proto` from `iter1-foundation`; can start as soon as `df-proto` exists. Tested against an in-process scripted control plane (tonic server) and local Git repositories.
- New dependencies: agent-client-protocol, rmcp, notify (cert reload).
- Git hook commands and Git config entries from `Assign.workspace` are not applied in iteration 1 (the control plane sends none).
