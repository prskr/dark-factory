# Tasks

> Groups 1–3 (mechanical summary and framing) do not depend on `factory-minion` (DF-8) and can ship first. Groups 4–6 start after `factory-minion` is implemented.

## 1. Tool inputs in the mechanical fold

- [ ] 1.1 Write failing `df-db` tests first: `continuation_events` returns per tool event the kind, the location paths and the raw input cut to 4,096 characters; update events return only the fields they set. Then extend the query and `ContinuationRow` and regenerate `.sqlx/`. Verify: `cargo nextest run -p df-db transcript` (container-backed).
- [ ] 1.2 Write failing unit tests first in `df-agent-channel` (`summary` tests) for the input rules of design D1: one case per kind in the table, workspace paths made relative, 160-byte cut on a character boundary with `…`, line breaks to spaces, input left out when the title contains it, last values of update events win. Then implement the input extraction as a pure function. Verify: `cargo nextest run -p df-agent-channel summary`.
- [ ] 1.3 Write failing tests first for the scenarios "Command of an older call", "Paths of an edit", "Long input" and "Completed calls stay individual". Then change the tool line format and remove `collapse_completed`. Verify: `cargo nextest run -p df-agent-channel summary`.

## 2. Recent window and drop order

- [ ] 2.1 Write failing `df-db` tests first for a query that returns the final `content` and `rawOutputJson` of given tool calls (by run, turn and tool call id) of one task. Then implement it and regenerate `.sqlx/`. Verify: `cargo nextest run -p df-db transcript` (container-backed).
- [ ] 2.2 Write failing unit tests first: the recent window is the last 2 `(run_id, turn_id)` groups; events without a turn id belong to the turn before; output head 512 and tail 1,536 bytes with the cut marker; diffs as `diff <path> (+a -b lines)`; window output above 24 KiB loses the oldest outputs first. Cover the scenarios "Output in the recent window" and "Diff in the recent window". Then implement. Verify: `cargo nextest run -p df-agent-channel summary`.
- [ ] 2.3 Write failing unit tests first for the drop order of design D3 and the scenarios of "Continuation summary size cap" (including "Recent outputs kept before old messages are dropped" and "Recent outputs dropped before user messages"). Then rewrite `fit_entries`. If `summary.rs` passes 1,500 lines, split it into `summary/{fold,input,window,fit}.rs`. Verify: `cargo nextest run -p df-agent-channel summary` and `cargo xtask check-file-length`.
- [ ] 2.4 Wire the second query into `continuation_summary` and add an assign test with two runs that checks the scenarios "Tool calls in the summary" and "Output in the recent window" end to end through `Assign`. Verify: `cargo nextest run -p df-agent-channel assign` (container-backed).

## 3. Framing in the runtime

- [ ] 3.1 Write failing tests first in `factory-agent` for the scenarios "Summary framed as data" and "Summary tries to close the block" (case-insensitive tag removal). Then change `build_acp_prompt_blocks` (design D4). Verify: `cargo nextest run -p factory-agent harness`.
- [ ] 3.2 Update the docs of the mechanical part and the framing: `docs/content/using/chats.md` (what the `context_compacted` notice shows), `docs/content/architecture/agent-runs.md` (summary content and framing), `crates/factory-agent/docs/claude-acp.md` if it describes the prepended block, and `design/agent-run.md` (continuity paragraph). Verify: `just docs-build`.

## 4. Strategy setting (after `factory-minion` is implemented)

- [ ] 4.1 Write failing migration and `df-db` tests first: `projects.continuation_summary` defaults to `mechanical`, accepts `minion`, rejects other values; saving a project stores and audits it. Then add the migration and queries, regenerate `.sqlx/`. Verify: `cargo nextest run -p df-db projects` (container-backed).
- [ ] 4.2 Write failing `df-web` tests first for the scenarios of "Continuation summary strategy of a project" (default, opting in with audit, hint when the minion is off). Then add the field to the project form next to `subtask_merge`. Verify: `cargo nextest run -p df-web project`.
- [ ] 4.3 Write failing assign tests first for "Default project" and "Minion selected but off". Then resolve the effective strategy in `assign.rs` (design D5). Verify: `cargo nextest run -p df-agent-channel assign` (container-backed).

## 5. Stored minion summary (after `factory-minion` is implemented)

- [ ] 5.1 Write failing `df-db` tests first for `task_summaries`: upsert stores text and `covers_through`; the guarded upsert keeps the higher `covers_through` ("Older job finishes last"); rows are deleted with the task. Then add the migration and queries, regenerate `.sqlx/`. Verify: `cargo nextest run -p df-db task_summaries` (container-backed).
- [ ] 5.2 Write failing `df-db` tests first for the scenarios "Every end path enqueues" (report, cancel, lost sandbox, `end_overdue`) and "Mechanical project enqueues nothing", plus: no job while the minion is off. Then enqueue `minion.continuation_summary` `{task_id}` in `runs::apply_end` (design D6). Verify: `cargo nextest run -p df-db runs` (container-backed).
- [ ] 5.3 Write failing `df-minion` tests first with the fake `ModelCall` for the scenarios of "Incremental continuation summary": the previous summary is in the data block, the update prompt is used, the previous summary is never dropped at the input cap. Then extend the call input. Verify: `cargo nextest run -p df-minion summary`.
- [ ] 5.4 Write failing tests first for the outbox handler in `df-agent-channel` (fake `ModelCall`): "Summary updated at run end", "Two quick run ends" (no call), "Next run end catches up", events of a run that is not ended are left out, `Ok` without a call for a deleted task, a `mechanical` project or the minion off, and the retry and give-up statuses of "Continuation summary job retries". Then implement the handler (design D6), add the dependency `df-agent-channel → df-minion` and update `docs/architecture.md`. Verify: `cargo nextest run -p df-agent-channel summary` (container-backed) and `cargo xtask check-crates`.
- [ ] 5.5 Write a failing test first for "Summary jobs do not delay titles" and "Restart during a summary job". Then add the third `Dispatcher` in `dark-factory/src/outbox.rs` with only `minion.continuation_summary` (lease 330 s, batch 2, give-up 1 h) and add the kind to the known kinds of every dispatcher (`with_known_kinds`, DF-8 D8). Verify: `cargo nextest run -p dark-factory outbox` (container-backed).
- [ ] 5.6 Write failing assign tests first for "Stored summary used", "Events after the stored summary", "No stored summary yet" and "Assign does not wait for a model". Then render the minion form of design D7 and add the counter `df.continuation.summaries{strategy, source}`. Verify: `cargo nextest run -p df-agent-channel assign` (container-backed).

## 6. Docs and checks

- [ ] 6.1 Update the published docs for the strategy: the project settings section in `docs/content/using/setup.md` (setting `continuation_summary`, values, default, effect, that it sends transcript content to the minion's provider), `docs/content/using/chats.md` and `docs/content/architecture/agent-runs.md` (stored summary, run-end job, fallback), the metric `df.continuation.summaries` in `docs/content/observability/telemetry.md`, and `product/context.md` F3 (the stored summary now exists). Verify: `just docs-build`.
- [ ] 6.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete` and `openspec validate --all --strict`. Verify: all pass; list the container-backed tests that could not run.
