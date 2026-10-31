# factory-agent configuration reference

`factory-agent run` reads its configuration from environment variables. The values are read once at
startup (`config.rs`).

## Environment variables

| Variable | Required | Default | Effect |
|----------|----------|---------|--------|
| `DF_CONTROL_PLANE_ADDR` | yes | none | Address of the control plane gRPC endpoint. `host:port` is accepted; `https://` is prepended when no `http://` or `https://` scheme is given. |
| `DF_CONTROL_PLANE_SERVER_NAME` | yes | none | TLS server name checked against the control plane certificate. |
| `DF_TLS_DIR` | no | `/run/factory/tls` | Directory with `ca.crt` (instance CA), `tls.crt` and `tls.key` (pod identity). Re-read on every connection attempt, so rotated certificates are picked up on reconnect. |
| `DF_DATA_DIR` | no | `/data` | Persistent volume. The runtime uses `repo/` (workspace checkout), `home/` (harness `$HOME`, session logs) and `runtime/` (outbox segments, `run.json`). |
| `DF_RUN_DIR` | no | `/run/factory` | Ephemeral runtime directory. Holds `runtime.sock` and the `wip.index` scratch file. Also exported to the harness. |
| `DF_RUNTIME_SOCK` | no | `/run/factory/runtime.sock` | Path of the runtime Unix socket. Read by the `mcp` and `git-credential` subcommands (also available as `--socket`), which the harness and git start as child processes. `run` creates the socket at `$DF_RUN_DIR/runtime.sock` and exports that path to its children. |
| `DF_GIT_ALLOW_PLAIN_HTTP` | no | unset | `true` lets git clone and push over `http://` (besides `https://`). Read once at startup, before the harness starts. The control plane sets it from the chart value `forge.allowPlainHttp` (default off); the token then travels unencrypted, so use it for throwaway clusters only. Any other value keeps git `https`-only. |
| `DF_LOG_FORMAT` | no | `text` | Log format of `run`: `text` (human-readable) or `json` (one JSON object per line). Also available as `--log-format`. Any other value makes the process exit before it starts. The control plane sets it to its own format. |
| `RUST_LOG` | no | `info` | Log level in `EnvFilter` syntax, for example `info` or `info,factory_agent=debug`. Unset, empty or invalid values log at `info`; an invalid value also logs a warning naming `RUST_LOG`. The control plane sets it from `sandbox.logLevel`. DEBUG adds agent message and thought chunks. |

A missing required variable makes `run` log `invalid configuration` and exit with status 1.

## Other inputs

| Input | Source | Effect |
|-------|--------|--------|
| `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` (and lowercase) | environment | Forwarded to git and the harness. |
| `GIT_SSL_CAINFO` | environment | Forwarded to git when no CA path is configured by the runtime. |

## Fixed settings

These are compiled in and not configurable.

| Setting | Value | Effect |
|---------|-------|--------|
| Agent `core.hooksPath` | `/opt/factory/hooks-empty` | Read-only empty hooks directory from the image. |
| Outbox size | 256 MiB, segments of 16 MiB | Soft cap: senders wait when it is full; a message already taken from the queue is still stored at shutdown. |
| Wait for `Assign` | 10 min | `run` exits with status 1 when the control plane sends no `Assign`. |
| Handshake steps | 15 s each | Connect, stream open and `HelloAck`; then backoff and retry. |
| Git commands | 5 min each | A clone, fetch or push that takes longer is killed and fails prepare or finalize. |
| Credential request | 10 s | Wait for a `CredentialGrant`. Pending requests older than this are not re-sent after a reconnect. |
| Factory tool call | 30 min | Wait for a `ToolCallResult`; then the tool call returns an error. |
| Harness exit grace | 5 s | Time to exit after the ACP connection closed, then it is killed. |

## Startup and shutdown behavior

- **Outbox.** The outbox opens with the state stored in `$DF_DATA_DIR/runtime/run.json`. The run id in
  `HelloAck` or `Assign` decides whether that state is kept or discarded.
- **Message order.** `HarnessCredentials` must arrive before `Assign`. Credentials that arrive later are
  ignored and reported as a `runtime.harness_credentials_late` notice. A `Cancel` that arrives before
  `Assign` ends the run as `CANCELLED` without preparing the workspace.
- **Idle wait.** After a turn that ended normally with no queued prompt (and no `complete_task`,
  rate limit, budget limit, cancel or crash), the runtime reports `AWAITING_INPUT` and keeps the
  harness session up. A `Prompt` starts the next turn in the same session (reported as `RUNNING`),
  `Cancel` ends the run `CANCELLED`, and `Release` or the end of the wall-clock budget ends it
  `END_TURN`. If the turn had a blocking request (a `ToolCallResult` with `await_input_after_turn`,
  `ask_question` with `blocking: true`), `Release` ends the run `BLOCKED` instead; the budget end
  still gives `END_TURN`. `complete_task` in the same turn wins and ends the run `COMPLETED`. The runtime has no idle timer of its own: the control plane decides when to send
  `Release` (`runs.keepAliveSeconds`). A lost stream does not end the wait; the runtime reconnects.
- **Shutdown.** After the final `RunReport` is queued, the runtime keeps trying to deliver queued
  messages: it flushes them over the open stream, or reconnects, for up to about 10 s plus a 5 s
  acknowledgement wait. Messages that are not acknowledged stay in the outbox and a warning is logged.
- **Credentials.** Git is told to send the repository path (`credential.useHttpPath`), and credentials
  are only released for the assigned repository. Cached credentials are dropped when git reports them as
  rejected (`git credential erase`) and when the grant's `expires_at` is near.
