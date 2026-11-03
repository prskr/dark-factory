## 1. Fold function

- [x] 1.1 Unit tests for the fold: text messages and chunk merging, tool call merge by `(run_id, turn_id, tool_call_id)` with last title and status (same id in two turns stays two lines), hidden blocks left out of user entries
- [x] 1.2 Pure fold in `df-agent-channel/src/summary.rs` with `EventKind`; header rendered from the task
- [x] 1.3 Failing tests: consecutive `completed` calls collapse into `Tool: <n> calls completed`; a single completed call keeps its own line; failed, pending and in-progress calls stay individual and break a streak
- [x] 1.4 Failing tests: drop order at the cap (tool lines and agent messages oldest first, user messages only when nothing else is left, newest entry always kept, order preserved)
- [x] 1.5 Implement collapsing and the drop order; cap 64 KiB including the 4 KiB header; `fold_summary` takes events and a budget only (no previous entries, no split/join)

## 2. Remove the stored summary

- [x] 2.1 Delete migration `20261103000002_task_summaries.sql` (never on `main`), `update_task_summary`, `stored_summary_text`, `stored`, `store_summary`, split/join helpers and their tests
- [x] 2.2 Remove the summary update at run end from `session.rs` and its tests

## 3. Build on `Assign`

- [x] 3.1 Failing DB tests (`df_test_postgres`): reconnect mid-run excludes the assigned run's events and keeps earlier runs'; a run that ended without a report is included; more than 500 events are all read; tool calls in the summary
- [x] 3.2 `Assign` queries all events of the task with the four kinds and `run_id <> <assigned run>`, ordered by `id`, selecting only the payload fields the fold uses; keep "Follow-up run" green

## 4. Docs and checks

- [x] 4.1 `design/agent-run.md` continuity paragraph: rebuilt per `Assign` without the assigned run, tool lines with collapsed streaks, 64 KiB cap with user messages kept first
- [x] 4.2 F3 in `product/context.md` and `roadmap.md`: input, output, trigger as in design D6; the stored summary with a cursor comes with F3; chats (iteration 3) reuse this summary
- [x] 4.3 `product/iterations/iteration-2.md`: list `iter2-continuation-summary`
- [x] 4.4 Clippy, `cargo fmt`, `cargo-machete`; state which container-backed tests could not be run
