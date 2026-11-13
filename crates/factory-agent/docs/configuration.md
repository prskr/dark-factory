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
| `DF_TLS_DIR` | no | `/run/factory/tls` | Directory with `ca.crt` (instance CA), `tls.crt` and `tls.key` (pod identity). Re-read on every connection attempt, so rotated certificates are picked up on reconnect; each read also refreshes the copy of `ca.crt` in `$DF_RUN_DIR/ca/`; if the copy fails, the attempt backs off and retries instead of connecting. With `DF_LANDLOCK=enforce` the harness can't read this directory. Git trusts the copy of `ca.crt` only for the gateway's Git proxy (`http.<proxy URL>.sslCAInfo`: `-c` on the clone, then `.git/config`); other hosts keep the system trust store. |
| `DF_DATA_DIR` | no | `/data` | Persistent volume. The runtime uses `repo/` (workspace checkout), `home/` (harness `$HOME`, session logs) and `runtime/` (outbox segments, `run.json`). Under Landlock the harness gets full access to `repo/` and `home/` and none to `runtime/`. |
| `DF_RUN_DIR` | no | `/run/factory` | Ephemeral runtime directory. Holds `runtime.sock`, the `wip.index` scratch file and `ca/ca.crt` (copy of the instance CA, the only file of the directory the harness reads under Landlock). Also exported to the harness. |
| `DF_RUNTIME_SOCK` | no | `/run/factory/runtime.sock` | Path of the runtime Unix socket. Read by the `mcp` subcommand (also available as `--socket`), which the harness starts as a child process. `run` creates the socket at `$DF_RUN_DIR/runtime.sock` and exports that path to its children. |
| `DF_LANDLOCK` | no | `enforce` | Filesystem isolation of the harness: `enforce` starts every harness through `factory-agent sandbox` in a Landlock domain and fails the run (notice `sandbox.landlock_unavailable`) on a kernel below Landlock ABI 6; `disabled` starts the harness unrestricted and logs a warning at every run start. Any other value makes `run` exit with an error naming the variable. The control plane sets it from `sandbox.landlock`. Like every `DF_*` variable except `DF_RUNTIME_SOCK`, `DF_RUN_DIR` and `DF_LOG_FORMAT`, it is removed from the harness environment in both modes. See [Sandbox filesystem isolation](../../security/sandbox.md). |
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
- **Harness files.** Before the adapter starts, the runtime writes every file of
  `Assign.harness.acp.files` below `/data/home`, replacing what is there and creating missing
  directories as real directories (a symlink in their place is replaced, never followed). A path
  that is absolute or has a `..` component fails the run before the adapter starts. The runtime
  writes no other harness file: the system prompt (`.claude/CLAUDE.md` for `claude-code`,
  `.config/opencode/AGENTS.md` for `opencode`) and Claude Code's `.claude/settings.json` are
  files of the list.
- **Harness environment.** The harness starts with the environment of `Assign` (`AcpHarness.env`):
  the gateway's inference endpoint as base URL, the placeholder API key `df-placeholder` and the
  instance CA file, or, for `opencode`, a configuration document in `OPENCODE_CONFIG_CONTENT` and
  the CA in `NODE_EXTRA_CA_CERTS`; for `claude-code` that is `ANTHROPIC_BASE_URL`, `CLAUDE_CODE_OAUTH_TOKEN` (provider
  scheme `claude-subscription`) or `ANTHROPIC_API_KEY` (other schemes), `ANTHROPIC_MODEL` (the persona's model ID), and `NODE_EXTRA_CA_CERTS`
  (`/run/factory/ca/ca.crt`), plus `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` so Claude Code sends
  no telemetry or update checks past the endpoint. The runtime holds no model credential and reports no token usage: the
  gateway measures it, and usage the harness reports over ACP is not forwarded.
- **Harness confinement.** With `DF_LANDLOCK=enforce`, the runtime probes the kernel before it writes
  the harness files; below Landlock ABI 6 it ends the run `FAILED` with a `sandbox.landlock_unavailable`
  notice and starts nothing. Otherwise it starts the adapter command as
  `factory-agent sandbox -- <command>`, which applies the Landlock domain to itself and replaces
  itself with the command (exit status 125, command not run, on any setup failure, including a
  symlink in the path of `repo/` or `home/`). The domain grants read and execute on every top-level
  directory that is not a symlink, except those holding `$DF_DATA_DIR`, `$DF_RUN_DIR` and
  `$DF_TLS_DIR`; full access to `repo/`, `home/`, `/tmp` and `/dev`; read access to
  `$DF_RUN_DIR/ca/`; and scopes signals and abstract Unix sockets. In both modes the harness
  environment has no `DF_*` variable except `DF_RUNTIME_SOCK`, `DF_RUN_DIR` and `DF_LOG_FORMAT`
  (the wrapper removes them with `enforce`, the runtime itself with `disabled`); the harness
  inherits only stdin, stdout and stderr. At startup, before anything below `$DF_DATA_DIR` is
  created or the outbox opens, the runtime fails if `$DF_DATA_DIR/runtime` is a symlink or has a
  symlink in its path, removes symlinks below it, and replaces files that have more than one hard
  link with a copy. Full description: [Sandbox filesystem isolation](../../security/sandbox.md).
- **Prompt input.** The control plane releases queued messages together in one `Prompts` control
  message, in queue order, also after a reconnect. The runtime queues the whole batch before waking
  and starts one turn with every waiting prompt. Each message starts with its `author_name` and a
  colon, for example `Peter Kurfer:` or `Request decision:`, even for a single message. An empty
  name adds no author line. A continuation summary stays before the messages; hidden text blocks
  still reach the harness. Deploy the runtime and control plane from the same release: the old
  singular `Prompt` control message is not supported.
- **Idle wait.** After a turn that ended normally with no queued prompt (and no `complete_task`,
  rate limit, budget limit, cancel or crash), the runtime reports `AWAITING_INPUT` and keeps the
  harness session up. A `Prompts` batch starts the next turn in the same session (reported as `RUNNING`),
  `Cancel` ends the run `CANCELLED`, and `Release` or the end of the wall-clock budget ends it
  `END_TURN`. If the turn had a blocking request (a `ToolCallResult` with `await_input_after_turn`,
  `ask_question` with `blocking: true`), `Release` ends the run `BLOCKED` instead; the budget end
  still gives `END_TURN`. `complete_task` in the same turn wins and ends the run `COMPLETED`. The runtime has no idle timer of its own: the control plane decides when to send
  `Release` (`runs.keepAliveSeconds`). A lost stream does not end the wait; the runtime reconnects.
- **Interrupt.** `Interrupt { by, turn_id }` ends the current turn only. `by` is the display name
  of the board member, `turn_id` the id of the turn the control plane saw open (`turn_N`, the
  `turn_id` of its `TurnStarted`). The runtime ignores an `Interrupt` for any other turn, outside a
  turn, and after a reconnect (it is never re-sent). Otherwise it sends the ACP notification
  `session/cancel` and waits up to 5 seconds for the response to the outstanding `session/prompt`.
  Dark Factory tool calls of the turn that wait for a result, and those the harness starts until the
  turn ends, complete at once with a cancelled result (`Interrupted by <by>`; the latter are not
  sent to the control plane). A late `ToolCallResult` for a cancelled call is dropped, with one
  exception: `end_run_after_turn` still applies, because the control plane already completed the
  task. A late `await_input_after_turn` is ignored.
  - If the harness answers with stop reason `cancelled`, the runtime records `TurnEnded` with stop
    reason `CANCELLED`, detail `Interrupted by <by>` and `interrupted_by = <by>`. The run and the
    harness session stay. With queued prompts the next turn starts, otherwise the runtime reports
    `AWAITING_INPUT` as after a normal turn end. The next turn's input starts with "Your previous
    turn was interrupted by <by>." before the prompts, and the interrupted turn counts toward
    `max_turns`.
  - If the response is ready when the interrupt arrives, or has another stop reason (for example
    `end_turn`, `max_tokens`), the turn is not interrupted: it is recorded with its real stop
    reason, no `interrupted_by`, and the usual outcome logic applies.
  - If the harness fails the cancel or does not answer within 5 seconds, the turn ends with stop
    reason `ERROR` and no `interrupted_by`, and the run ends `FAILED`. A rate-limit error as the
    response ends the turn `RATE_LIMITED` and the run `RATE_LIMITED`.
  - `Cancel` and the budgets win over an interrupt.
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
- **Hidden paths.** `Workspace.hidden_paths` (globs of the project's push policy, `globset` syntax
  without `{}`) are applied as a non-cone sparse checkout. The generated `.git/config` always sets
  `core.sparseCheckout = true` and `core.sparseCheckoutCone = false`, and `.git/info/sparse-checkout`
  is rewritten at prepare and at finalize: `/*`, then `!/<glob>` per hidden glob and, unless the
  glob ends in `/**`, `/<glob>/` to bring directories that the glob does not name back. Without
  hidden paths the file is `/*`, a full checkout. A fresh clone writes the file before the first
  checkout, so no blob below a hidden path is fetched; a reused workspace runs
  `git sparse-checkout reapply`. Then prepare removes the files below hidden globs that are still in
  the working tree: tracked files whether or not `reapply` kept them (it keeps modified ones),
  untracked and ignored files. It resets their index entries, runs `reapply` again and emits one
  `workspace.hidden_paths_removed` notice (level `WARNING`) listing the paths, truncated after 50.
  A path with a symlink among its parent directories is skipped. Restoring the WIP snapshot of a
  fresh workspace restores only the paths that differ between `HEAD` and the snapshot and match no
  hidden glob, so no blob below a hidden path is fetched. The WIP snapshot resets hidden paths to
  `HEAD`. An invalid glob fails prepare. Requires Git 2.27 or newer.
- **Task branch pushes and the rewrite policy.** `Workspace.task_branch` is the task's stored branch
  (`factory/task/<KEY>`, or an existing branch). After every turn whose `HEAD` differs from the last
  pushed commit the runtime pushes `HEAD:refs/heads/<task_branch>`. The runtime keeps the commit the
  remote branch had when it last fetched (prepare) or pushed it in `$DF_DATA_DIR/runtime/remote_sha`
  (written next to `run.json`, which the harness cannot write; it is kept across runs). It never
  uses `refs/remotes/origin/<branch>` for this, because the agent's own `git fetch` moves that ref.
  With `Workspace.rewrite_policy = REWRITE_POLICY_WITH_LEASE`, a `HEAD` that does not contain
  `remote_sha` is pushed with `--force-with-lease=refs/heads/<branch>:<remote_sha>`; any other push
  is a plain fast-forward. `NEVER` and `UNSPECIFIED` never force. A lease or non-fast-forward
  rejection (`stale info`, `[rejected]`) is reported as `REJECTED_NON_FAST_FORWARD` and not retried
  with force. The WIP snapshot branch is the only force push (`--force`).
- **Existing workspace and rewrites.** Prepare fetches and compares the local with the remote task
  branch: equal, local ahead (pushed, plainly), remote ahead (fast-forwarded). If both have commits
  the other lacks, the branch counts as diverged (`workspace.diverged`), except under
  `WITH_LEASE` when the fetched remote is still the commit in `remote_sha`: then the local history
  was rewritten by an earlier, killed run, and prepare pushes it with lease. A diverged prepare
  keeps the stored `remote_sha`, so a later rewrite that drops the other side's commits is rejected
  and a second prepare reports the divergence again. The file is written without following links
  (`remote_sha` is replaced, not written through).
- **Existing branches.** With `Workspace.existing_branch` a fresh workspace checks out
  `origin/<task_branch>`. If the remote lacks it, prepare fails with `the branch `<name>` does not
  exist on the remote` (`PrepareError::BranchMissing`), nothing is pushed, and the run ends `FAILED`
  with that reason.
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
