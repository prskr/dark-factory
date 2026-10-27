## 1. Fold function

- [ ] 1.1 Write failing unit tests for `fold_summary(previous_entries, events, cap)`: text messages and chunk merging (as today), tool call merge by `(run_id, turn_id, tool_call_id)` with last title and status (same id in two turns stays two lines), hidden blocks left out of user entries, oldest-first dropping at the cap, entries without the separator, split/join round trip
- [ ] 1.2 Implement `fold_summary` in `df-agent-channel/src/assign.rs` (or a new `summary.rs`), reusing `fit_summary` semantics
- [ ] 1.3 Add `tool_call` and `tool_call_update` to the event kinds read; keep the text extraction and chunk merging checks of the old tests; the "last 20 messages" assertion is replaced by the byte-cap retention

## 2. Storage

- [ ] 2.1 Migration `task_summaries (task_id PK → tasks ON DELETE CASCADE, covers_through_event_id, text, updated_at)`
- [ ] 2.2 Failing DB test (`df_test_postgres`): update reads only events after the cursor and advances it; an update with an older cursor does not overwrite a newer row
- [ ] 2.3 Implement `update_task_summary(pool, task)` in `df-agent-channel`; regenerate `.sqlx` metadata

## 3. Wiring

- [ ] 3.1 Failing test: run end stores the summary; a failing update is logged and does not fail the run end
- [ ] 3.2 Call the update on the run-report path in `session.rs`
- [ ] 3.3 Failing test: a run that ended without a summary update (stale or missing row) still has its events in the next `Assign` summary
- [ ] 3.4 `Assign` runs `update_task_summary` and reads the row (replaces `transcript_summary`'s direct query); keep the existing scenario "Follow-up run" green

## 4. Docs

- [ ] 4.1 `design/agent-run.md` continuity paragraph: stored summary, tool lines, update at run end
- [ ] 4.2 F3 in `product/context.md`: input (events after a cursor incl. tool calls, previous summary), output (summary text and cursor), trigger (run end or idle), and that mid-run compaction is out of scope (design D7)
- [ ] 4.3 `product/iterations/iteration-2.md` and `roadmap.md`: list `iter2-continuation-summary`; roadmap iteration 3 chats reuse the stored summary
- [ ] 4.4 Clippy, `cargo fmt`, `cargo-machete`; state which container-backed tests could not be run in the sandbox
