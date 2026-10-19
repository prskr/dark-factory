# Tech Debt

Known shortcomings that were deferred on purpose. Each entry says what is wrong, when it matters, and how to fix it.

- **Fixing an entry:** add a test that fails without the fix, then delete the entry in the same change.
- **Adding an entry:** use the next free ID. IDs are never reused. The next free ID is TD-8.
- Origin: code review of the iteration-1 agent runtime (2026-10-19). The `TD-n (Rn)` headings give the review IDs, which commit messages and code comments may refer to.

## Summary

| ID   | Sev    | Area | Issue |
|------|--------|------|-------|
| TD-1 | medium | `factory-agent` git | One fixed 5 min timeout for every git command, including clone |
| TD-2 | medium | `factory-agent` connection | 10 s shutdown flush window is shorter than a control-plane restart |
| TD-3 | low    | `factory-agent` workspace | Diverged workspace: every turn sends a rejected push and a PushReport |
| TD-4 | low    | `factory-agent` outbox | Failed `run.json` save in `acknowledge` leaves segment accounting inconsistent |
| TD-5 | low    | `factory-agent` tests | Shutdown-while-disconnected test relies on a 100 ms sleep |
| TD-6 | low    | `factory-agent` connection | Timed-out tool calls are still re-sent after a reconnect |
| TD-7 | low    | `factory-agent` tests | Several behaviors have no test |

## Entries

### TD-1 (R62). One fixed git timeout for clone and push

`crates/factory-agent/src/git.rs` (`GIT_TIMEOUT`, `Git::run_cmd_in`)

Every git command runs with `GIT_TIMEOUT` (5 min) and is killed when it expires. For a large repository, the blobless clone or the blob fetch during `checkout -B` can take longer than that. Prepare would then fail on every run, and the limit cannot be changed.

**Fix:** Make the limit configurable (for example `DF_GIT_TIMEOUT`, documented in `crates/factory-agent/docs/configuration.md`), or give clone and fetch a longer limit than local commands. An inactivity timeout (no output for N minutes) would be more precise, but it needs more code.

### TD-2 (R63). Shutdown flush window shorter than a control-plane restart

`crates/factory-agent/src/connection.rs` (`SHUTDOWN_RECONNECT_TIMEOUT`, `should_stop`), `crates/factory-agent/src/main.rs` (30 s wait for the connection task)

After shutdown, the connection manager tries to reconnect for 10 s, and `main` waits at most 30 s. One connection attempt alone can take up to 3 × 15 s (`HANDSHAKE_TIMEOUT` for connect, stream open and HelloAck). If a control-plane restart takes longer, the final RunReport stays only in the outbox on the PVC, and nothing replays it once the pod has exited.

**Fix:** This needs a protocol decision. Either the control plane works out the outcome of a run whose pod exited without a RunReport (for example by reading `/data/runtime/outbox` at the next run, or by treating the run as FAILED/retryable), or the flush window becomes configurable and longer. Record the decision in `design/agent-run.md`.

### TD-3 (R64). Diverged workspace pushes and reports on every turn

`crates/factory-agent/src/runner.rs` (`initial_last_pushed_sha`), `crates/factory-agent/src/workspace.rs` (`push_after_turn`)

A diverged workspace starts with `last_pushed_sha = None`. `push_after_turn` then tries a non-force push after every turn and at finalize. Each attempt sends a `RejectedNonFastForward` PushReport. The remote is never changed, so the behavior is correct, but it adds noise.

**Fix:** Skip `push_after_turn` while `prepare_diverged` is set. The divergence is already reported once, by the `workspace.diverged` notice and `RunReport.workspace.diverged`.

### TD-4 (R65). Failed `run.json` save leaves segment accounting inconsistent

`crates/factory-agent/src/outbox.rs` (`Outbox::acknowledge`)

`acknowledge` removes acked segments from `closed_segments` / `active_segment` before `save_run_state()?`. If the save fails, the segment files stay on disk but are no longer tracked, and `total_bytes` is not reduced. After a restart, `open` rescans the directory and the problem goes away.

**Fix:** Collect the segments to delete without changing the in-memory lists, save, then delete the files and update the lists.

### TD-5 (R66). Shutdown-while-disconnected test relies on a 100 ms sleep

`crates/factory-agent/src/tests.rs` (`test_review_r40_final_report_is_delivered_after_shutdown_while_disconnected`)

The test sleeps 100 ms so that the manager notices the dropped stream. If the manager is still in the active session at that point, it takes the other shutdown path and the test fails. AGENTS.md: avoid timing-based tests.

**Fix:** Wait on a condition instead, for example an observable "disconnected" state on the handle, or the mock control plane seeing the stream close.

### TD-6 (R67). Timed-out tool calls are still re-sent after a reconnect

`crates/factory-agent/src/runner.rs` (`handle_mcp_call_tool`, `TOOL_CALL_TIMEOUT`), `crates/factory-agent/src/connection.rs` (`CommandFilter.pending_tool_calls`)

When a tool call times out (30 min), the runner removes it from its own map, but `CommandFilter` keeps it and re-sends it on every reconnect. The control plane may run the tool again, although nothing waits for the result anymore. Credential requests had the same problem; there the filter now expires entries by age.

**Fix:** Expire tool-call entries in `CommandFilter` after `TOOL_CALL_TIMEOUT`, the same way credential requests are expired.

### TD-7 (R68). Behaviors without a test

`crates/factory-agent/src/tests.rs`

These behaviors were verified by reading the code only:

- A failed WIP restore on a fresh clone fails prepare (`prepare_workspace`). Without this, finalize would delete the remote WIP branch.
- The agent's `core.hooksPath` is `/opt/factory/hooks-empty` (`main.rs`, `HARNESS_HOOKS_PATH`).
- The runtime socket keeps accepting after an accept error (`RuntimeSocketServer::spawn`).
- Timeouts on git commands, on waiting for `Assign`, and on factory tool calls.
- A diverged workspace starts with no pushed SHA (`run_session`).
- After a driver error, `last_pushed_sha` and the push reports of completed turns are kept (`run_session`).
- Directory creation errors at startup exit with a failure (`main.rs`).
- Socket connection handlers are owned by a `JoinSet` and dropped with the accept task.

The most valuable tests are the WIP restore, because it prevents data loss, and the timeouts and diverged start SHA. Those can be reached with `tokio::time::pause()` and the existing diverged-repo fixtures.

## Accepted decisions

These were reviewed and kept on purpose. Don't raise them as issues unless the stated assumption changes.

- **TurnEnded vs. trailing transcript chunks:** ordering is guaranteed. `agent-client-protocol` 3.4.0 (`concepts::ordering`) waits for each notification handler to finish before it routes the next message. The handler awaits the `outbound_tx` send, so all chunks of a turn are queued before the prompt response arrives. Revisit if the ACP crate changes its dispatch model.
- **Late `HarnessCredentials`:** the documented order is "credentials before Assign". If credentials arrive after Assign, the runtime sends a notice and does not use them.
- **Git config denylist instead of an allowlist:** the denylist was extended. Replace it with an allowlist once the set of config keys the control plane sends is known.
- **Credential requests without a path:** they are denied. `credential.useHttpPath` is set in `.git/config` and through `-c` on every runtime git call, so git always sends the path.
- **Credential request expiry:** `CommandFilter` expires credential requests by age, using the same 10 s constant as the runner (`CREDENTIAL_REQUEST_TIMEOUT`).
- **Harness exit:** stdin is closed when the ACP transport is dropped. The runtime waits 5 s, then kills the harness.
- **Fake agent cancel flag:** `fake-acp-agent` resets `cancelled` at the end of a prompt, because a cancel can be handled before the spawned prompt task starts.
- **Hooks path not configurable:** `/opt/factory/hooks-empty` is a constant. It matches the runtime image layer (`deploy/BUILD.bazel`, `/hooks-empty` in the image mounted at `/opt/factory`). Generated commit hooks (`/run/factory/git-hooks`, `design/agent-run.md` §7.1) are not implemented yet.
