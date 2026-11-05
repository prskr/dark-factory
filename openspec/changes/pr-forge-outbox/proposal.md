# Proposal

## Why

Opening the draft PR, marking it ready and posting the completion summary call the forge inline from the agent session, best effort (TD-91). A forge hiccup or a control plane restart at the end of a completed run loses "mark ready" and the summary for good: the PR stays a draft until a human notices. The same inline design causes TD-11 (a stale summary can be posted again), TD-12 (a completed run without a push tries to open a PR), TD-13 (concurrent creation can mark an unrecorded PR ready) and TD-82 (unbounded pooled connections held across forge calls). `iter3-webhooks` added the first PR outbox job (`pr.mark_draft`); the other PR actions should follow the same pattern before the first real installation.

## What Changes

- PR forge actions become one durable outbox job kind, `pr.sync`, enqueued in the transaction that records the trigger:
  - a successful task-branch push ahead of base for a task without a PR (open the draft PR),
  - the end of a run that pushed the task branch (open the draft PR if it is still missing) or that ended `completed` (also mark ready and post that run's summary),
  - a board member marking a task done (mark ready, no summary).
- Failed forge calls are retried with backoff (30 s doubling to 10 min, given up after one hour with a warning), also across control plane restarts; no next push or run end is needed.
- A completed run that never pushed the task branch opens no PR (TD-12). **BREAKING** for the behavior only: today it tries and fails or opens an empty PR.
- The completion summary is stored per run (`runs.completion_summary`), so a run without its own `complete_task` call posts no summary (TD-11). Existing values are not copied; `tasks.completion_summary` stays unused until a later change drops it (expand/contract, ADR-0012).
- Ready and summary always target the PR recorded on the task (TD-13).
- The agent session no longer runs forge work; `FORGE_TIMEOUT` and the web UI's inline `mark_ready_bounded` go away. The divergence flag is set in the batch transaction.
- The outbox handler wiring moves from `lifecycle_events.rs` to its own module (TD-108).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `pull-request-lifecycle`: "Draft PR on first push ahead of base" no longer retries at the end of a run without a push; "Failed creation is retried" retries with backoff independent of further pushes and across restarts; "Ready for review on completion" retries forge failures, posts only the ended run's own summary and opens no PR for a run that never pushed; "Idempotent creation" adds the concurrent-creation case.

## Out of scope

- Exactly-once summary comments (listing PR comments before posting): a forge that accepts a comment but answers with an error gets the summary twice on retry. Rejected; at-least-once is acceptable for a comment.
- Recovering a PR action given up after one hour: the next push, run end or mark-done enqueues a new job. Rejected as a separate mechanism; the periodic forge reconcile of TD-115 would cover it if it lands.

## Impact

- `crates/df-agent-channel`: `pr.rs` turns into enqueue helpers plus a `pr.sync` outbox handler (replaces `on_push_report`, `on_run_end`, `ensure_draft`); `session.rs` drops `spawn_forge_work`/`FORGE_TIMEOUT` and enqueues in the batch transaction; `runs.rs` `end_run_tx` enqueues at run end; `tools.rs` `complete_task` writes the summary on the run.
- `crates/df-db`: `pr.sync` kind and payload next to `pr.mark_draft`; `mark_task_done` enqueues; migration for `runs.completion_summary`; `.sqlx` metadata.
- `crates/df-agent-channel/src/pr/mark_draft.rs`: takes the per-task advisory lock around its check and update.
- `crates/df-web`: mark-done no longer calls the forge (`mark_pr_ready`, `mark_ready_bounded` removed).
- `crates/dark-factory`: outbox handler wiring in its own module, registers the `pr.sync` handler.
- Docs: `docs/content/operations/troubleshooting.md` ("No pull request appears", PR stays a draft), `docs/content/observability/telemetry.md` (`df.outbox.jobs` kinds).
- Tech debt: deletes TD-11, TD-12, TD-13, TD-82, TD-91 and TD-108 from `docs/tech_debt.md`.
