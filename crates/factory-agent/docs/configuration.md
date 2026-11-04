---
title: Sandbox runtime configuration
linkTitle: Sandbox runtime
weight: 1
---
`factory-agent run` reads its configuration from environment variables. The values are read once at
startup (`config.rs`).

## Environment variables

| Variable | Required | Default | Effect |
|----------|----------|---------|--------|
| `DF_CONTROL_PLANE_ADDR` | yes | none | Address of the control plane gRPC endpoint. `host:port` is accepted; `https://` is prepended when no `http://` or `https://` scheme is given. |
| `DF_CONTROL_PLANE_SERVER_NAME` | yes | none | TLS server name checked against the control plane certificate. |
| `DF_TLS_DIR` | no | `/run/factory/tls` | Directory with `ca.crt` (instance CA), `tls.crt` and `tls.key` (pod identity). Re-read on every connection attempt, so rotated certificates are picked up on reconnect. Git trusts `ca.crt` only for the gateway's Git proxy (`http.<proxy URL>.sslCAInfo`: `-c` on the clone, then `.git/config`); other hosts keep the system trust store. |
| `DF_DATA_DIR` | no | `/data` | Persistent volume. The runtime uses `repo/` (workspace checkout), `home/` (harness `$HOME`, session logs) and `runtime/` (outbox segments, `run.json`). |
| `DF_RUN_DIR` | no | `/run/factory` | Ephemeral runtime directory. Holds `runtime.sock` and the `wip.index` scratch file. Also exported to the harness. |
| `DF_RUNTIME_SOCK` | no | `/run/factory/runtime.sock` | Path of the runtime Unix socket. Read by the `mcp` subcommand (also available as `--socket`), which the harness starts as a child process. `run` creates the socket at `$DF_RUN_DIR/runtime.sock` and exports that path to its children. |
| `DF_LOG_FORMAT` | no | `text` | Log format of `run`: `text` (human-readable) or `json` (one JSON object per line). Also available as `--log-format`. Any other value makes the process exit before it starts. The control plane sets it to its own format. |
| `RUST_LOG` | no | `info` | Log level in `EnvFilter` syntax, for example `info` or `info,factory_agent=debug`. Unset, empty or invalid values log at `info`; an invalid value also logs a warning naming `RUST_LOG`. The control plane sets it from `sandbox.logLevel`. DEBUG adds agent message and thought chunks. |

A missing required variable makes `run` log `invalid configuration` and exit with status 1.

## Other inputs

| Input | Source | Effect |
|-------|--------|--------|
| `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` (and lowercase) | environment | Forwarded to git and the harness. |

## Fixed settings

These are compiled in and not configurable.

| Setting | Value | Effect |
|---------|-------|--------|
| Agent `core.hooksPath` | `/opt/factory/hooks-empty` | Read-only empty hooks directory from the image. |
| Outbox size | 256 MiB, segments of 16 MiB | Soft cap: senders wait when it is full; a message already taken from the queue is still stored at shutdown. |
| Wait for `Assign` | 10 min | `run` exits with status 1 when the control plane sends no `Assign`. |
| Handshake steps | 15 s each | Connect, stream open and `HelloAck`; then backoff and retry. |
| Final report flush window | 10 min | After finalize the runtime keeps reconnecting until every outbox message, the `RunReport` included, is acknowledged; then it exits. If the window passes, it exits with the messages left in the outbox and the control plane later ends the run `sandbox_lost` and resumes the task. The window does not extend the run's wall-clock budget. |
| Git commands | 5 min each | A clone, fetch or push that takes longer is killed and fails prepare or finalize. |
| Git retries | 3, after 1 s, 5 s and 15 s | Clone, fetch and push that fail transiently are retried (see "Git retries" below). |
| Factory tool call | 30 min | Wait for a `ToolCallResult`; then the tool call returns an error. |
| Harness exit grace | 5 s | Time to exit after the ACP connection closed, then it is killed. |

## Startup and shutdown behavior

- **Outbox.** The outbox opens with the state stored in `$DF_DATA_DIR/runtime/run.json`. The run id in
  `HelloAck` or `Assign` decides whether that state is kept or discarded.
- **Message order.** A `Cancel` that arrives before `Assign` ends the run as `CANCELLED` without
  preparing the workspace.
- **Harness environment.** The harness starts with the environment of `Assign` (`AcpHarness.env`):
  the gateway's inference endpoint as base URL, the placeholder API key `df-placeholder` and the
  instance CA file; for `claude-code` that is `ANTHROPIC_BASE_URL`, `CLAUDE_CODE_OAUTH_TOKEN` (provider
  scheme `claude-subscription`) or `ANTHROPIC_API_KEY` (other schemes), and `NODE_EXTRA_CA_CERTS`
  (`/run/factory/tls/ca.crt`), plus `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` so Claude Code sends
  no telemetry or update checks past the endpoint. The runtime holds no model credential and reports no token usage: the
  gateway measures it, and usage the harness reports over ACP is not forwarded.
- **Idle wait.** After a turn that ended normally with no queued prompt (and no `complete_task`,
  rate limit, budget limit, cancel or crash), the runtime reports `AWAITING_INPUT` and keeps the
  harness session up. A `Prompt` starts the next turn in the same session (reported as `RUNNING`),
  `Cancel` ends the run `CANCELLED`, and `Release` or the end of the wall-clock budget ends it
  `END_TURN`. If the turn had a blocking request (a `ToolCallResult` with `await_input_after_turn`,
  `ask_question` with `blocking: true`), `Release` ends the run `BLOCKED` instead; the budget end
  still gives `END_TURN`. `complete_task` in the same turn wins and ends the run `COMPLETED`. The runtime has no idle timer of its own: the control plane decides when to send
  `Release` (`runs.keepAliveSeconds`). A lost stream does not end the wait; the runtime reconnects.
- **Shutdown.** After the final `RunReport` is queued, the runtime keeps trying to deliver queued
  messages: it flushes them over the open stream, or reconnects with the usual backoff (also when
  the stream drops during the flush, for example by a control plane restart), until all are
  acknowledged or the final report flush window of 10 minutes has passed. Messages that are not
  acknowledged by then stay in the outbox and a warning is logged. `main` waits for the connection
  task for the window plus 60 s for a connection attempt that is under way.
- **Git credentials.** The runtime holds no Git credentials: the gateway's Git proxy authenticates
  upstream. The generated `.git/config` sets an empty `credential.helper` (resets helpers from other
  config scopes) and `http.<proxy URL>.sslCAInfo` to the instance CA, scoped to
  `scheme://host:port/` of the workspace `repo_url` so other HTTPS remotes keep the system trust
  store; git runs with `GIT_TERMINAL_PROMPT=0`, so a
  rejected request fails at once instead of prompting. `credential.*` and `http.*` entries from the
  repository or `Workspace.git_config` are dropped.
- **Git retries.** A clone, fetch or push whose output shows HTTP 503 (`returned error: 503`) or a
  connection error (`Failed to connect to`, `Connection refused`, `Connection reset by peer`,
  `Empty reply from server`, and mid-transfer `Operation timed out`,
  `transfer closed with outstanding read data remaining`, `SSL_ERROR_SYSCALL`, `Recv failure`,
  `was not closed cleanly`) is retried up to 3 times. Only git's own lines count, not `remote:`
  lines from the server. A 403 together with the gateway's `remote: df: unknown source` (the gateway
  has no snapshot with the pod IP yet) is retried as well. Output with `[remote rejected]`,
  `[rejected]`, any other 403 or 502 (the forge rejected the proxy's credential) is never retried,
  nor is any other failure
  (DNS errors included: the proxy host is a `hostAliases` entry). Before a push is retried the
  runtime reads the remote ref with `git ls-remote`; if it already has the pushed commit (the
  response was lost after the ref update), the push counts as successful. Lazy blob fetches of the
  blobless clone (during checkout) are not retried.
