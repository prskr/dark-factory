# Tech Debt

Known shortcomings that were deferred on purpose. Each entry says what is wrong, when it matters, and how to fix it.

- **Fixing an entry:** add a test that fails without the fix, then delete the entry in the same change.
- **Adding an entry:** use the next free ID. IDs are never reused. The next free ID is TD-61.
- Origin of TD-1 to TD-7: code review of the iteration-1 agent runtime (2026-10-19). The `TD-n (Rn)` headings give the review IDs, which commit messages and code comments may refer to.
- Origin of TD-8 to TD-26: code review of `iter1-run-orchestration` (`df-agent-channel`, 2026-10-19).
- Origin of TD-27: removal of remote build execution (2026-10-20).
- Origin of TD-28 to TD-36: code review of `iter1-sandbox-provisioning` (`df-k8s`, 2026-10-21). The `TD-n (Rn-Nm)` headings give the IDs used in that review.
- Origin of TD-41: external review of the `run-network-policy` spec (2026-10-22).
- Origin of TD-38 to TD-40: code review of `iter1-integration-plugins` (`df-plugins`, 2026-10-22). The `TD-n (ID)` headings give the IDs used in that review.
- Origin of TD-42 to TD-43: code review of `iter1-control-plane-web` (`df-web`, 2026-10-22). The `TD-n (ID)` headings give the IDs used in that review.
- Origin of TD-44: code review of `ui-redesign` (`df-web`, 2026-10-22). The `TD-n (ID)` headings give the IDs used in that review.
- Origin of TD-45 to TD-47: code review of `ui-polish` (`df-web`, `df-agent-channel`, 2026-10-23). The `TD-n (ID)` headings give the IDs used in that review.
- Origin of TD-49 to TD-50: code review of `iter1-egress-gateway` (`df-gateway`, 2026-10-23). The `TD-n (ID)` headings give the IDs used in that review.
- Origin of TD-51: first cluster run of `iter1-egress-gateway` (2026-10-24).
- Origin of TD-52: code review of `agent-session-logging` (`factory-agent`, `dark-factory`, `df-k8s`, 2026-10-24). The `TD-n (ID)` headings give the IDs used in that review.
- Origin of TD-55: code review of `otel-metrics-and-tracing` (`factory-agent`, 2026-10-24). The `TD-n (ID)` headings give the IDs used in that review.
- Origin of TD-56: code review of `network-management-ui-polish` (`df-web`, 2026-10-25). The `TD-n (ID)` headings give the IDs used in that review.
- Origin of TD-57 to TD-58 (both resolved the same day): code review of `harness-images` (`df-k8s`, `df-agent-channel`, `deploy/images`, 2026-10-26). The `TD-n (ID)` headings give the IDs used in that review.
- Origin of TD-59: dev cluster verification of `harness-images` (2026-10-26), task 7.4 carried over from `iter1-egress-gateway`.
- Origin of TD-60: CodeRabbit review of PR #26 (`harness-images`, 2026-10-26). The `TD-n (ID)` headings give the IDs used in that review.

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
| TD-8 | medium | `df-agent-channel` scheduler | Timeout and Gone handling await provider teardown inline in the scheduler loop |
| TD-9 | low    | `df-agent-channel` session | Sandbox teardown after a RunReport is awaited inline in the session loop |
| TD-10 | low   | `df-agent-channel` server | Streams opened during shutdown start new sessions |
| TD-11 | low   | `df-agent-channel` PR | `completion_summary` is never cleared and can be posted again |
| TD-12 | low   | `df-agent-channel` PR | A completed run without a push still tries to open a PR |
| TD-13 | low   | `df-agent-channel` PR | Concurrent PR creation can mark an unrecorded PR ready |
| TD-14 | low   | `df-agent-channel` session | Two extra database round trips per batch |
| TD-15 | low   | `df-agent-channel` runs | Guarded run update + `RunChanged` publish exists in three copies |
| TD-16 | low   | `df-agent-channel` tests | `eventually` test helper polls with `sleep` and ends in `panic!` |
| TD-17 | medium | `df-agent-channel` session | Tool-call handlers have no timeout; drain timeouts disagree with the shutdown budget |
| TD-18 | medium | `df-agent-channel` scheduler | A run whose provisioning was interrupted is never provisioned again |
| TD-19 | low   | `df-agent-channel` storage | Run, task and tool-call SQL is runtime-checked and partly duplicates `df-db` |
| TD-20 | low   | Helm chart | `agent.tls.clientCaFile` needs `ca.crt`, which an external issuer may not fill |
| TD-21 | low   | Helm chart | `terminationGracePeriodSeconds` equals `SHUTDOWN_GRACE` (30 s) |
| TD-22 | low   | `df-agent-channel` runs | `pod_ip` is not stored when `Hello` arrives before `Started` |
| TD-23 | low   | `df-agent-channel` scheduler | `enforce_timeouts` loads every active run on every tick |
| TD-24 | low   | `df-agent-channel` tests | Lock-wait test is not scoped to its own database |
| TD-25 | low   | `dark-factory` main | Log message interpolates `component` |
| TD-26 | low   | `df-agent-channel`, `dark-factory` | Small duplicate helpers and indirections |
| TD-27 | medium | Bazel | No remote build execution: container tests cannot run in agent sandboxes |
| TD-28 | medium | `df-k8s` provider | A transient resolution error at provision time leaves the run without the target's host alias |
| TD-29 | low   | `df-k8s` provider | A named target port without a ready endpoint skips the target until endpoints appear |
| TD-30 | low   | `df-db`, `df-k8s` | Nothing publishes `egress_policy`; DB changes reach policies only through the 30 s resync |
| TD-31 | medium | `df-k8s` events | Image-volume pull failures (runtime and harness) produce no `Gone` event |
| TD-32 | low   | `df-k8s` events | Watcher and resolver state is lost across a control-plane restart or leader change |
| TD-33 | low   | `.sqlx` | Offline metadata files come from two sqlx-cli versions |
| TD-34 | low   | `df-core`, Helm chart | Runtime image tag is written in two places |
| TD-35 | low   | `df-core` config | Default `issuer_name` does not match the chart's ClusterIssuer name |
| TD-36 | low   | `df-k8s` tests | No live test asserts a running sandbox; the shared live config uses `alpine` as the runtime image |
| TD-38 | low    | `df-plugins` forge client | Every `ForgeClient` method repeats resolve → retry → run → error mapping |
| TD-39 | low    | `deploy` image test | Shipped plugin component is not checked with `wasm-tools validate` |
| TD-40 | low    | `df-plugins` host | Import allow-list duplicates WIT world `plugin-base` by hand |
| TD-41 | medium | `df-k8s` tests | No live test that a sandbox reaches an allowed target through its ClusterIP |
| TD-42 | low    | `df-db` / `df-web` storage | Web UI queries are runtime-checked `sqlx::query_as`, not `query!` macros |
| TD-43 | low    | `df-web` transcript | Code blocks are rendered without syntax highlighting (syntect, design D3) |
| TD-44 | low    | `df-web` styles | Text contrast ratios (≥ 4.5:1) are documented, not tested |
| TD-45 | low   | `df-agent-channel` session | Turn state read with raw SQL; event kinds matched as strings |
| TD-46 | low   | `df-web` statuses | Task statuses enumerated in parallel and compared as strings |
| TD-47 | low   | `df-web` components | Message box and composer "+" use one-off class strings |
| TD-49 | low   | `df-gateway` reasons | Flow reasons and rule scopes are scattered string literals |
| TD-50 | low   | `df-gateway` tests | Proxy shutdown tests wait on the origin without a timeout |
| TD-51 | low   | `df-gateway` resolver | IP family preference is fixed to IPv4 first |
| TD-52 | medium | `factory-agent` logging | Tool call `input` is logged at INFO and can carry credentials the agent typed |
| TD-55 | low   | `factory-agent` timings | `RuntimeTiming` measurement boilerplate repeated per operation |
| TD-56 | low   | `df-web` egress log | Client-side behaviour of the traffic page is only checked through markup strings |
| TD-59 | low   | `df-gateway` dev cluster | CONNECT with a mismatching TLS SNI was never checked against the deployed gateway |
| TD-60 | low   | `df-web` project form | Adapter command is split on whitespace; an argument containing a space does not survive an edit |

## Entries

### TD-1 (R62). One fixed git timeout for clone and push

`crates/factory-agent/src/git.rs` (`GIT_TIMEOUT`, `Git::run_cmd_in`)

Every git command runs with `GIT_TIMEOUT` (5 min) and is killed when it expires. For a large repository, the blobless clone or the blob fetch during `checkout -B` can take longer than that. Prepare would then fail on every run, and the limit cannot be changed.

**Fix:** Make the limit configurable (for example `DF_GIT_TIMEOUT`, documented in `crates/factory-agent/docs/configuration.md`), or give clone and fetch a longer limit than local commands. An inactivity timeout (no output for N minutes) would be more precise, but it needs more code.

### TD-2 (R63). Shutdown flush window shorter than a control-plane restart

`crates/factory-agent/src/connection.rs` (`SHUTDOWN_RECONNECT_TIMEOUT`, `should_stop`), `crates/factory-agent/src/main.rs` (30 s wait for the connection task)

After shutdown, the connection manager tries to reconnect for 10 s, and `main` waits at most 30 s. One connection attempt alone can take up to 3 × 15 s (`HANDSHAKE_TIMEOUT` for connect, stream open and HelloAck). If a control-plane restart takes longer, the final RunReport stays only in the outbox on the PVC, and nothing replays it once the pod has exited.

**Fix:** This needs a protocol decision. Either the control plane works out the outcome of a run whose pod exited without a RunReport (for example by reading `/data/runtime/outbox` at the next run, or by treating the run as FAILED/retryable), or the flush window becomes configurable and longer. Record the decision in `design/agent-run.md`.

**Decision (2026-10-21):** the runtime keeps flushing for up to 10 minutes after finalize, and a pod that exits without a report ends `sandbox_lost` and is resumed automatically. Implemented by `openspec/changes/iter2-run-resilience` (design D5).

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

### TD-8. Provider teardown blocks the scheduler loop

`crates/df-agent-channel/src/scheduler.rs` (`enforce_timeouts`, Gone handling), `crates/df-agent-channel/src/runs.rs` (`finish_run`)

`finish_run` awaits `provider.teardown`, and `enforce_timeouts` calls it for each timed-out run in sequence, inside the `select!` loop. Sandbox events, task notifications and the shutdown signal wait until all teardowns return. This contradicts the doc comment on `Scheduler::run`. It matters when many runs time out at once and the provider's delete is slow (Kubernetes API under load).

**Fix:** End the run in the database inline, run the teardown in the scheduler's existing provisioning `JoinSet` (bounded by a timeout), the same way provisioning was moved off the loop.

### TD-9. Session teardown is awaited inline

`crates/df-agent-channel/src/session.rs` (`run_effects`, `RunReport` branch)

After a `RunReport` the session awaits `runs::teardown` before it reads the next message. A slow provider delays the remaining acks of the ended run. Impact is small because the session ends right after; tool calls spawned from the same batch still store their results.

**Fix:** Run the teardown in the session's task set, like the forge work after a `RunReport`.

### TD-10. Streams opened during shutdown start new sessions

`crates/df-agent-channel/src/server.rs` (`serve`, `AgentService::stop_sessions`, `accept_loop`)

`stop_sessions` clears the sessions before tonic stops accepting streams and sends GOAWAY, and the TLS acceptor is only aborted after the server returns. A runtime that reconnects in that window starts a session that keeps graceful shutdown waiting until `SHUTDOWN_GRACE` (30 s) ends and `abort_all` runs.

**Fix:** Refuse new `Connect` calls once shutdown started (a flag checked in `connect`, returning `UNAVAILABLE`), and stop the acceptor first.

### TD-11. Stale completion summary

`crates/df-agent-channel/src/pr.rs` (`on_run_end`, `completion_summary`), `crates/df-agent-channel/src/tools.rs` (`complete_task`)

`tasks.completion_summary` is set by `complete_task` and never cleared. A later run that ends `completed` without its own `complete_task` call posts the earlier summary again. Today a run normally ends `completed` only through `complete_task`, which overwrites the value, so this is unlikely.

**Fix:** Clear the summary after posting it, or store it per run (`runs.completion_summary`) and read the ended run's value.

### TD-12. PR opened for a completed run without a push

`crates/df-agent-channel/src/pr.rs` (`on_run_end`, condition `pushed || completed`)

When a run ends `completed` and the task has no PR, `try_ensure_draft` runs even if the task branch was never pushed. The forge rejects the request (branch missing or no diff) and a warning is logged on every such completion, or it opens an empty PR.

**Fix:** Require `task_branch_pushed` (from this or an earlier run) before creating a PR; decide whether a completed run with no changes needs a PR at all and record that in `specs/pull-request-lifecycle/spec.md`.

### TD-13. Concurrent PR creation

`crates/df-agent-channel/src/pr.rs` (`try_ensure_draft`, `record_pr`)

`on_push_report` and `on_run_end` run as separate forge tasks. If both see `pr_number` as `NULL` and the forge does not return `Conflict` for the second create, two PRs exist. `record_pr` keeps the first, but `on_run_end` marks its own, unrecorded PR ready and posts the summary there. Forgejo rejects a second PR for the same head and base, so this needs a forge that does not.

**Fix:** After `record_pr`, return the PR number stored in the database (`RETURNING pr_number`) and use that; optionally close the duplicate.

### TD-14. Extra round trips per batch

`crates/df-agent-channel/src/session.rs` (`Session::active`, `persist_batch`)

Each batch loads the full run row (`active`) and reads `last_persisted_seq` once outside and once inside the transaction. A runtime that sends one transcript chunk per batch causes four or more queries per message.

**Fix:** One `SELECT state, last_persisted_seq FROM runs` before preparing blobs serves both checks; keep the `FOR UPDATE` read in the transaction.

### TD-15. Duplicated run update and publish

`crates/df-agent-channel/src/runs.rs` (`set_run_state_tx`, `mark_started`, `mark_connected`, `notify_run`)

Three places update a run with a state guard and publish `RunChanged` (`mark_started` inline, `mark_connected` via `notify_run`, `set_run_state_tx`). A change to the guard or notification semantics has to be made in each.

**Fix:** Route `mark_started` and `mark_connected` through one helper that takes the update and publishes `RunChanged`, and inline `notify_run`.

### TD-16. `eventually` helper polls with `sleep`

`crates/df-agent-channel/tests/agent_port.rs` (`eventually`)

The helper checks a condition 200 times with a 25 ms `sleep` and ends with a hand-written `panic!`. AGENTS.md: avoid timing-based tests and hand-written panic guards. The 5 s budget can be too short on a loaded remote executor.

**Fix:** Wait on a notification (`Notifier` subscription for `RunChanged`/`TaskChanged`) or on the scripted client's received messages, with one overall `tokio::time::timeout`; fail with `assert!`.

### TD-17. Tool-call timeouts and drain budgets

`crates/df-agent-channel/src/session.rs` (`run_effects`, `DRAIN_TIMEOUT` = 60 s, `FORGE_TIMEOUT` = 60 s), `crates/df-agent-channel/src/server.rs` (`SESSION_DRAIN` = 20 s), `crates/dark-factory/src/main.rs` (`SHUTDOWN_GRACE` = 30 s)

Tool-call handlers call the forge without a timeout, while forge work after a `PushReport`/`RunReport` is bounded by `FORGE_TIMEOUT`. A hanging forge keeps a claimed `tool_calls` row without a result until the stream ends; re-sent calls get "still in progress" for 5 minutes, then "did not finish". At shutdown the session believes it has 60 s to drain, but `stop_sessions` aborts after 20 s, so a call that needs longer loses its result although the effect may have happened.

**Fix:** Bound each tool handler by `FORGE_TIMEOUT` and store a timeout error as the result. Derive the session drain from one budget that fits into `SHUTDOWN_GRACE` (and TD-21).

### TD-18. Interrupted provisioning is not retried

`crates/df-agent-channel/src/scheduler.rs` (`run`, `SHUTDOWN_DRAIN`, `provision_or_fail`)

If a replica stops (rollout beyond the 10 s drain, crash) between `start_queued_runs` and `provider.provision`, the run stays `pending`/`provisioning` without a sandbox. After `connectTimeoutSeconds` (10 min) it ends as `connect_timeout` and the task becomes `failed`; the user has to queue a message to retry.

**Fix:** On each tick, re-provision runs that have been `pending` longer than a short threshold and have no sandbox (provider calls must be idempotent, which `ensure_task_volume`/`provision` are meant to be), or end them as `provisioning_failed` with the task re-queued.

**Decision (2026-10-21):** re-provisioned by the scheduler tick. Implemented by `openspec/changes/iter2-run-resilience` (design D4).

### TD-19. Runtime-checked SQL outside `df-db`

`crates/df-agent-channel/src/runs.rs`, `tools.rs`, `transcript.rs`, `pr.rs`, `session.rs`

Run, task, tool-call and transcript persistence uses `sqlx::query(...)` strings (some built with `concat!`), not the compile-time checked `query!` macros and `.sqlx/` metadata that `df-db` uses. A column rename compiles and fails only at runtime; the tests against PostgreSQL catch it, plain builds do not. `transcript::insert` repeats the update of `df_db::mark_prompt_delivered` (now with an extra `task_id` scope).

**Fix:** Move the queries into `df-db` as `query!`/`query_as!` functions generic over `PgExecutor`, regenerate `.sqlx/`, and give `mark_prompt_delivered` the `task_id` scope so `transcript` can call it.

### TD-20. Client CA file depends on the issuer

`deploy/helm/dark-factory/templates/configmap.yaml` (`agent.tls.clientCaFile: /etc/dark-factory/tls/ca.crt`), `values.yaml` (`tls.ca.existingIssuer`)

`serve` refuses to start without readable `agent.tls` files. The chart's own CA issuer (`tls.ca.create=true`, default) fills `ca.crt`; an external issuer set through `tls.ca.existingIssuer` may not, and the control plane then crash-loops.

**Fix:** Document in the chart that `existingIssuer` must be a CA-type issuer that fills `ca.crt`, or add a value for a separate client CA secret.

### TD-21. Termination grace period equals the shutdown budget

`crates/dark-factory/src/main.rs` (`SHUTDOWN_GRACE` = 30 s), control-plane Deployment (no `terminationGracePeriodSeconds`, Kubernetes default 30 s)

The kubelet sends SIGKILL at the same moment the supervisor gives up. Today the session drain (20 s) and scheduler drain (10 s) fit, so nothing is lost; any longer drain would be cut off by SIGKILL instead of the supervisor.

**Fix:** Set `terminationGracePeriodSeconds` in the chart a few seconds above `SHUTDOWN_GRACE` (for example 40).

### TD-22. `pod_ip` missing when the runtime connects first

`crates/df-agent-channel/src/runs.rs` (`mark_started`)

`mark_started` only writes `pod_ip` while the run is `pending`/`provisioning`. If `Hello` is processed before the provider's `Started` event, the run is already `preparing` and `pod_ip` stays `NULL`. Authentication does not use the column (`SandboxProvider::resolve_identity`), so only diagnostics are affected.

**Fix:** Store `pod_ip` with `COALESCE` for any non-terminal run, and change state only from `pending`/`provisioning`.

### TD-23. Timeout scan loads every active run

`crates/df-agent-channel/src/scheduler.rs` (`enforce_timeouts`), `crates/df-agent-channel/src/runs.rs` (`list_active_runs`)

Every 15 s tick on every replica loads all non-terminal runs and filters the deadlines in Rust. Fine for tens of runs; wasteful for hundreds with several replicas.

**Fix:** Filter overdue runs in SQL (`started_at`/`connected_at` plus interval) and keep the Rust check for the final decision.

### TD-24. Lock-wait test is not scoped to its database

`crates/df-agent-channel/src/runs.rs` (`message_queued_while_the_run_ends_is_not_stranded`)

The test waits for `pg_stat_activity WHERE wait_event_type = 'Lock'` across the whole cluster. A lock wait in another parallel test database ends the wait early; the test then passes without exercising the race (it does not fail spuriously).

**Fix:** Add `AND datname = current_database()`.

### TD-25. Log message interpolates a value

`crates/dark-factory/src/main.rs` (`supervise_listeners`)

`error!("{component} listeners still running after the grace period")` (and the `info!` on shutdown) put a value into the message. AGENTS.md (Logging) asks for structured fields.

**Fix:** `error!(component, "listeners still running after the grace period")`.

### TD-26. Small duplicates and indirections

`crates/df-agent-channel/src/session.rs` (`stop_requested`), `crates/dark-factory/src/main.rs` (`stopped`), `crates/df-agent-channel/src/scheduler.rs` (`Scheduler::start_runs`), `crates/df-agent-channel/src/testing.rs` (`anyhow_free`)

`stop_requested` and `stopped` do the same `watch` wait but treat a dropped sender differently (never stop vs. stop). `start_runs` only forwards to `start_queued_runs`; `anyhow_free` is an alias module.

**Fix:** One shared helper with the dropped-sender behavior stated in its doc; inline the wrapper and the alias.

### TD-27. No remote build execution: container tests cannot run in agent sandboxes

`.bazelrc`, `.bazelrc.agents`, `MODULE.bazel`, `build/platforms/BUILD.bazel`

RBE was removed in favor of local static musl builds with an optional remote cache. Agent sandboxes have no container socket, so `df-test-postgres` based tests only run in CI and on developer machines. ADR-0023, `design/agent-run.md`, `design/egress.md` and `product/context.md` still assume RBE runs them.

**Fix:** Self-hosted RBE workers with a controlled image and an exec C toolchain that does not depend on the client, see [remote-execution.md](remote-execution.md) §3.

### TD-28 (R9-N1). Transient resolution error at provision time drops a target for the whole run

`crates/df-k8s/src/provider.rs` (`ClusterTargetResolver::resolve`, `Transient` branch without a cached entry)

If a cluster target cannot be resolved for a transient reason (timeout, 5xx, connection error) and there is no last good resolution yet, the rule is skipped. During provisioning that means the pod starts without the target's `hostAliases` entry and `NO_PROXY` entry. `hostAliases` cannot change after the pod starts, so the run cannot reach the service by name, even after the reconcile pass adds the NetworkPolicy rule. The failure is closed but only logged as a warning. This matters for the first run of a project after a control-plane restart, when the cache is empty.

**Fix:** Let `provision` fail with a retryable error when a target resolves `Transient` without a cached entry, so the run is retried instead of starting degraded. The reconcile path keeps skipping the rule.

### TD-29 (R9-N2). Named target port without a ready endpoint is skipped

`crates/df-k8s/src/provider.rs` (named `targetPort` lookup in the EndpointSlices, classified `Transient`)

A Service whose `targetPort` is a name resolves only through a ready EndpointSlice port. While the target's pods are not ready, the resolution is transient; without a cached entry the rule is skipped. The EndpointSlice watch triggers a new pass when endpoints appear, so access returns. A run provisioned in that window has the TD-28 problem.

**Fix:** Covered by the TD-28 fix for provisioning. For reconcile, the current behavior is acceptable.

### TD-30 (R9-N3). No writer for the `egress_policy` notification

`crates/df-db/src/notify.rs` (`Notification::EgressPolicy`), `crates/df-db/migrations/20261017000001_initial_schema.sql` (only `egress_policy_version_seq`), `crates/dark-factory/src/main.rs` (notify subscriber)

The reconcile loop listens for `EgressPolicy`, but nothing sends it: there is no trigger and no `NOTIFY` on allowlist changes. A changed cluster-service entry reaches running policies only through the 30 s drift resync, which is still within the spec's 30 s bound.

**Fix:** Send the notification when allowlist entries change. This belongs to `iter1-egress-gateway` (the allowlist writer); delete this entry there.

### TD-31 (R5-N5). Image-volume pull failure produces no `Gone`

`crates/df-k8s/src/provider.rs` (`map_pod_event`)

A runtime or harness image (the two image volumes of a sandbox) that cannot be pulled keeps the pod in `ContainerCreating`. The failure is not in the container statuses, so no `Gone` event is emitted. The run waits until the connect timeout (10 min).

**Fix:** Watch the pod's Events (reason `Failed`/`ErrImagePull` for the image volume), or treat a pod that stays in `ContainerCreating` beyond a limit as `Gone` with a reason.

### TD-32 (R5-N10). State lost across a restart or leader change

`crates/df-k8s/src/provider.rs` (`EventTracker`, `ClusterTargetResolver` last-good cache), `crates/df-agent-channel/src/scheduler.rs` (no startup resync)

The event tracker knows only objects seen by the current process. A pod or Sandbox that vanished while the control plane was down gets no `Gone`; the run ends through the connect timeout or the hard stop. The resolver's last-good cache also starts empty after a restart or on a new leader, so a transient error drops a rule that a long-running leader would have kept. Documented in `crates/df-k8s/README.md`.

**Fix:** At startup (and on becoming leader), reconcile the active runs from the database against the Sandbox store and end runs whose Sandbox is missing.

### TD-33 (R5-N18). Mixed `.sqlx` metadata format

`.sqlx/`

Only `query-3aad8b60….json` has the `origin` fields of sqlx-cli 0.9.0; the other files come from an older version. Both work.

**Fix:** Regenerate all metadata with one pinned sqlx-cli version (`docs/development.md`, "Updating `sqlx` Offline Metadata").

### TD-34. Runtime image tag written twice

`crates/df-core/src/config.rs` (default `runtime_image`), `deploy/helm/dark-factory/values.yaml` (`image.runtime.tag`)

The tag (`0.1.0`) is hard-coded in both. The chart always renders its value, so the df-core default only applies outside the chart. A release bump must change both.

**Fix:** Derive one from the other at build time (for example the chart's `appVersion`), or drop the df-core default and make the value required.

### TD-35. Default issuer name does not match the chart

`crates/df-core/src/config.rs` (default `issuer_name: "dark-factory-ca-issuer"`), `deploy/helm/dark-factory/templates/certs.yaml` (`<release-ns>-<fullname>-ca-issuer`)

The chart always sets the issuer name, so deployments are not affected. Running the binary without the chart, or tests built on `Default::default()`, reference a ClusterIssuer that does not exist; the sandbox's csi volume then never mounts.

**Fix:** Make `issuer_name` required (no default), or document that it must match the ClusterIssuer the chart creates.

### TD-36. Live tests never reach a running sandbox

`crates/df-k8s/src/provider.rs` (`test_provider_live_cluster_integration`), `crates/df-k8s/src/reconcile.rs` (`test_reconciliation_live_cluster`, `policy_loop_live_reverts_manual_edit_and_adds_new_target`)

`live_config()` (`crates/df-k8s/src/test_support.rs`) delivers `alpine:latest` as the runtime image, which has no `/opt/factory/factory-agent`, so the container cannot start. Only `test_reconciliation_live_cluster` overrides it with `runtime:dev`, and it checks the policy only. None of the live tests asserts `Running` or a `Started` event. The pod-start path is only covered by unit tests with a seeded store.

**Fix:** Use the real runtime image (published for the dev cluster) in one live test and assert the `Started` event with the pod IP.

### TD-38 (S7). Duplicated call pipeline in `PluginForgeClient`

`crates/df-plugins/src/forge.rs` (`impl ForgeClient for PluginForgeClient`)

All five methods repeat `resolve` → `with_retries` → `host.run` → `convert::…` / `convert::from_error`. A new method or capability (an issue tracker plugin) copies it again, and a change to retries or error mapping must touch every copy.

**Fix:** One private `call(project, |plugin, store| …)` helper that does resolve, retry, run and error mapping; the methods only build the guest call and convert the result.

### TD-39 (R2-P1). Shipped plugin component is not validated by a test

`deploy/test_images.sh`, `deploy/BUILD.bazel` (`forgejo_plugin`)

The image test checks only the size (< 1 MiB) and the `\0asm` magic of `/plugins/forgejo.wasm`. `wasm-tools validate` ran once by hand (2026-10-22: 338 KB, valid). `wasm-tools` is not in the Bazel multitool. A broken opt transition or toolchain change only shows when `serve` loads the component.

**Fix:** Add `wasm-tools` to multitool and run `wasm-tools validate` (optionally check the exports) in the image test.

### TD-40 (R2-S2). Import allow-list duplicates `plugin-base`

`crates/df-plugins/src/host.rs` (`ALLOWED_IMPORTS`), `contract/wit/integrations/integrations.wit` (world `plugin-base`)

The host lists the allowed interfaces by hand. When `plugin-base` gains an import and the list is not updated, components built against the contract are refused at load.

**Fix:** A test that resolves world `plugin-base` (for example with `wit-parser`, or a component built against it) and asserts that every import is in `ALLOWED_IMPORTS`.

### TD-41. ClusterIP reachability of cluster targets is untested

`crates/df-k8s/src/reconcile.rs` (`test_reconciliation_live_cluster`, `policy_loop_live_*`), `crates/df-k8s/src/render.rs`

`hostAliases` map target names to the Service's ClusterIP, while the generated `NetworkPolicy` allows the backing pods (selector) or endpoint addresses (`/32` `ipBlock`). That only works if the CNI evaluates egress policy after Service translation. The live tests compare the rendered policy with the expected one, but none opens a connection from a sandbox pod to an allowed and to a non-allowed ClusterIP.

**Matters when:** a new CNI, an eBPF kube-proxy replacement or a CNI upgrade is rolled out.

**Fix:** Add a live test with a pod in the sandbox namespace, a target Service and a second non-target Service: connecting to the target's ClusterIP succeeds, to the other fails.

### TD-42 (S1). Web UI SQL is runtime-checked

`crates/df-db/src/{users,catalog,task_views}.rs`, `crates/df-web/src/auth/store.rs`

The queries added for the web UI use `sqlx::query_as` / `query_scalar` instead of `query!` / `query_as!`, so `.sqlx/` does not cover them. `catalog.rs` builds its project queries with the `select_project!` string macro, which the sqlx macros cannot take. A schema change that breaks one of these queries surfaces only in the container-backed tests or at runtime.

**Matters when:** a migration renames or retypes a column these queries read.

**Fix:** Write the queries as literal `query_as!` calls (inline the `select_project!` variants), regenerate `.sqlx/`. The session store may stay runtime-checked (it picks its SQL at runtime).

### TD-43 (P7). No code highlighting in the transcript

`crates/df-web/src/transcript/mod.rs` (Markdown rendering)

Design D3 and the proposal name syntect classes for code blocks; the implementation renders Markdown with comrak only, so code blocks are unhighlighted. No spec requirement depends on it.

**Matters when:** users read longer diffs or code in the transcript on a phone.

**Fix:** Run comrak with a syntect adapter emitting CSS classes, add the theme to `styles/input.css`; or remove syntect from design D3 via `/opsx:update`.

### TD-44 (C5). Contrast ratios are not tested

`crates/df-web/styles/input.css` (color tokens)

The web-ui-shell spec requires a text contrast of at least 4.5:1 (3:1 for text of 24 px and larger) in both themes. The ratios are only listed in `docs/design-system.md`; no test computes them from the tokens, so a token change can break the requirement unnoticed.

**Matters when:** a color token is changed or a theme is added.

**Fix:** Add a unit test that parses the foreground/background token pairs for light and dark from `styles/input.css` and asserts each contrast ratio is at least 4.5:1 (3:1 for large text).

### TD-45 (S5). Turn state read with raw SQL in the channel crate

`crates/df-agent-channel/src/session.rs` (`turn_open`, `persist_batch`)

`turn_open` queries `transcript_events` directly instead of going through `df-db`, and the event kinds `"turn_started"` / `"turn_ended"` are string literals in both the Rust code and the SQL.

**Matters when:** the transcript schema or the event kind names change; the hold-back then breaks without a compile error.

**Fix:** Move `turn_open` into `df-db` next to the transcript queries and name the kinds once (constants or an enum shared by both sites).

### TD-46 (S8, S35). Task statuses enumerated in parallel and compared as strings

`crates/df-web/src/config_pages.rs` (`Counts::columns`, `ProjectStatusCount.status`), `crates/df-web/src/task_pages/view.rs` (`queued_state`)

Statuses are listed separately in `StatusSet`, `StatusFilter` and `Counts`; `ProjectStatusCount.status` and `TaskDetail.status` are `String`s matched against literals such as `"failed" | "completed"`.

**Matters when:** a status is added or renamed; a missed site fails silently.

**Fix:** Parse statuses into `df_core::TaskStatus` at the DB boundary and derive the count columns from `StatusFilter` (label, `as_str`, group).

### TD-47 (S20). Message box and composer "+" use one-off classes

`crates/df-web/src/task_pages/view.rs` (message textarea, "Link issue" `+` button)

Both use hand-written class strings instead of a documented component; `docs/design-system.md` names the "Message input" pattern but no component for it.

**Matters when:** the message box or the composer is reused or restyled; the copies drift apart.

**Fix:** Add named components or constants in `components/`, each with a unit test, and document them in `docs/design-system.md`.

### TD-49 (S13). Flow reasons and rule scopes are scattered string literals

`crates/df-gateway/src/proxy.rs` (reasons such as `upstream-failed`, `private-destination`, `sni-mismatch`, `removed-from-allowlist`, `gateway-shutdown`), `crates/df-gateway/src/control/snapshot.rs` and `crates/df-db/src/egress.rs` (`"project"` / `"instance"` scope)

`Denial::reason()` covers only the policy denials; the other reasons are literals at their call sites, and the HTTP status is picked by comparing reason strings (S25). The scope is derived with the same `if project_id.is_some()` in two crates.

**Matters when:** a reason is added or renamed; the proto comment, the README table and the UI filters drift without a compile error.

**Fix:** One `FlowReason` enum (reason string, verdict, status) in `df-gateway` and a `Scope` enum in `df-db`.

### TD-50 (S27). Proxy shutdown tests wait on the origin without a timeout

`crates/df-gateway/src/proxy/tests.rs` (`listener.accept()` and the request-head read loop in `stopping_the_proxy_cuts_off_a_streaming_response_and_records_why`; the `accept` calls near lines 646 and 692)

The client-side reads are bounded by 5 s, the origin-side waits are not.

**Matters when:** the proxy stops forwarding after a regression; the tests hang instead of failing.

**Fix:** Wrap each origin-side wait in `tokio::time::timeout(Duration::from_secs(5), …)` like the client reads.

### TD-51. IP family preference is fixed to IPv4 first

`crates/df-gateway/src/resolve.rs` (`SystemResolver::new`)

The resolver uses `LookupIpStrategy::Ipv4thenIpv6` because the first cluster has no IPv6 egress. A dual-stack cluster can't prefer IPv6, and the proxy dials only the first resolved address, with no fallback to the other family.

**Matters when:** the cluster nodes get IPv6 egress and IPv6 should be preferred, or a host's first address is unreachable.

**Fix:** Add a `gateway.ipFamily` setting (`ipv4First` default, `ipv6First`, `ipv4Only`, `ipv6Only`) that maps to the hickory strategy, rendered by the chart and documented in the df-gateway README configuration table. Make the dial try the remaining resolved addresses in order (each under the upstream timeout budget) before it reports `upstream-failed`.

### TD-52 (S1). Tool call input can carry credentials

`crates/factory-agent/src/runner.rs` (`log_outbound`, `ToolCall` arm)

The spec requires the raw tool input (cut to 512 bytes) in the INFO `tool call` event. An agent command such as `curl -H "Authorization: …"` or `git clone https://user:tok@…` puts the token in the pod log at the default level. Accepted as a risk in the `agent-session-logging` design.

**Matters when:** pod logs are shipped to a store with wider access than the sandbox, or an agent handles long-lived credentials.

**Fix:** Spec change first: redact credential-looking patterns (auth headers, userinfo in URLs, `token=`) before logging, or log `input` at DEBUG only. Add a test with a token in the input.

### TD-55 (S6). RuntimeTiming measurement repeated per operation

`crates/factory-agent/src/runner.rs` (workspace prepare, harness start, turn, finalize)

Each timed operation repeats `let started = Instant::now(); let result = op().await; send_timing(&tx, Op, started, result.is_ok()).await;`.

**Matters when:** more operations are timed, or the timing message changes; a missed `send_timing` goes unnoticed.

**Fix:** One `async fn timed<T, E>(tx, op, fut) -> Result<T, E>` that sends the timing and returns the result; keep explicit calls only where the result needs extra handling (push).

### TD-56 (P6). Traffic page client behaviour has no runtime test

`crates/df-web/src/egress/log/page.rs` (Alpine state: pause/resume, divider drag/keys/resize, remembered width, details panel, Advanced badge)

Tests assert that strings appear in the rendered page. Nothing runs the script, and the obscura browser cannot stream SSE, so the live updates, the pause closing the stream, the clamping and the stored width were never exercised. Review rounds 2–4 each found a bug in this code that string tests did not catch (P10, P13, P14).

**Matters when:** the page script changes again, or htmx/Alpine are upgraded (P1 and P10 depend on internals of the vendored versions).

**Fix:** A browser test (for example Playwright against `just serve-local` with `--demo-flows`) for the spec scenarios Pause, Resume, Drag, Keyboard, Reset, Remembered width and Open/close details; run it in CI.

### TD-59. SNI mismatch not checked on a cluster

`crates/df-gateway/src/proxy.rs` (`sni-mismatch`)

The gateway closes a CONNECT tunnel whose ClientHello names another host than the allowed CONNECT target and logs it as `sni-mismatch`. Unit tests in `crates/df-gateway/src/proxy/tests.rs` cover it, but the check against the deployed gateway (`harness-images` task 7.4: `openssl s_client -proxy <gateway>:3128 -connect <allowlisted host>:443 -servername <other-name>` from a sandbox) was not done: a run ends before a manual request can be sent from its sandbox.

**Matters when:** the gateway, its network policy or the sandbox proxy setup changes, or before the egress controls are relied on for isolation.

**Fix:** Run the check from a sandbox that stays up (for example a fake-acp-agent mode that waits, or a debug pod with the sandbox labels in the sandbox namespace); verify the tunnel is closed before the destination is contacted and the event is logged as `sni-mismatch`. Then delete this entry.

### TD-60 (CR3). Adapter command loses argument boundaries

`crates/df-web/src/config_pages.rs` (`validate_project`, `project_values`)

The project form shows the stored `adapter_command` joined with spaces and parses the field with `split_whitespace`. An argument containing whitespace (`/workspace/my config.json`) is split into two arguments on save, and every later edit keeps it split. The persona field this replaces behaved the same way.

**Matters when:** a project adapter needs an argument with whitespace.

**Fix:** Use a reversible format for the field (one argument per line, or shell-style quoting with a small parser) and add a test that an edit keeps an argument containing a space.

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
- **Placeholder sandbox provider fails queued tasks:** `crates/dark-factory/src/unconfigured.rs` fails every provision, so queued tasks end as `provisioning_failed` instead of waiting forever. It stands in until `iter1-sandbox-provisioning` and `iter1-integration-plugins` are implemented. Don't deploy this build to an instance with queued tasks before then.
- **`RunPhase::AwaitingInput` maps to no run state:** `AwaitingInput` is a non-goal of `iter1-run-orchestration` (`design.md`, Non-Goals). Revisit with blocking questions and approvals.
- **At-most-once tool calls:** a crash between the forge call and storing its result loses the result; the call is not repeated (`crates/df-agent-channel/src/tools.rs`, spec `factory-tools`).
- **Session ends the stream once its run ended:** after a `RunReport` a relevant notification closes the stream. The runtime sends `RunReport` last (`crates/factory-agent/src/runner.rs`), it is acked before the effects run, and in-flight handlers are drained, so nothing is lost.
- **`Assign` may differ on reconnect:** `continuation.summary` is rebuilt from the transcript on every connect. The runtime uses only the first `Assign` per `run_id` (`CommandFilter`, `crates/factory-agent/src/connection.rs`).
