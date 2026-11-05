# Tasks

Most tests here are container-backed (`df_test_postgres`) and need a container socket; see `AGENTS.md`.

## 1. Job kind and per-run summary

- [x] 1.1 Write failing tests, then add the migration of design D4 (`runs.completion_summary TEXT NOT NULL DEFAULT ''`; `tasks.completion_summary` stays) and make `complete_task` (`crates/df-agent-channel/src/tools.rs`) write the summary on its run. Regenerate `.sqlx`. Verify with `cargo test -p df-agent-channel tools` (summary stored on the calling run, not on the task) and `cargo build --workspace`.
- [x] 1.2 Add `PR_SYNC_KIND = "pr.sync"` and `PrSyncJob { task_id, run_id, open, ready }` next to `MARK_DRAFT_KIND` in `crates/df-db/src/tasks.rs` (design D1). Verify with a serde round-trip unit test in `df-db`.

## 2. `pr.sync` handler

- [x] 2.1 Write failing tests, then add `PrSyncHandler` in `crates/df-agent-channel/src/pr/sync.rs` with steps 1–5 of design D1, moving `try_ensure_draft`, `record_pr`, `record_ready` and `new_pull_request` from `pr.rs` and building the context from the payload. Verify with `cargo test -p df-agent-channel pr::sync` against the fake forge: open creates and records a draft PR; a recorded PR is not opened again; a `completed`/`cancelled` task gets no PR; a cancelled task is not marked ready; ready marks the PR ready, writes `pr-ready:<n>` once across two runs of the job and posts the run's summary; an empty run summary posts no comment; a forge failure on create, ready or comment returns `Retry`; an invalid payload returns `GiveUp`; a ready-only job without a recorded PR returns `Retry` while an unfinished `open` job of the task exists and finishes without forge calls otherwise (step 4).
- [x] 2.2 Write a failing test, then make step 2 continue with the PR recorded in the database when `record_pr` updated no row (TD-13; scenario "Concurrent creation on a forge without conflict detection"). Verify with a test that records #7 before the job creates #8 and asserts ready and summary go to #7.
- [x] 2.3 Write a failing test, then skip "mark ready" when a message of the task has `queued_at` after the job's `ready` time, still posting the summary, checked under the per-task advisory lock together with the forge update (design D3; scenario "Reopened before the ready update"). Verify with tests for a message queued before (marked ready) and after (stays draft) the `ready` time.
- [x] 2.4 Write a failing test, then make `MarkDraftHandler` take the same per-task advisory lock around its status check and draft update (design D3). Verify with `cargo test -p df-agent-channel pr::mark_draft`: while a test holds the lock, the job waits; after the test marks the PR ready and releases it, the job drafts the PR.
- [x] 2.5 Add the `ponytail:` comment of design D5 at the advisory lock. Verify by reading it.

## 3. Enqueue in the trigger's transaction

- [x] 3.1 Write failing tests, then in the session's batch transaction enqueue `pr.sync { open, run_id }` for a task-branch push `Ok` with commits ahead when the task has no PR, and set `diverged` for `RejectedNonFastForward` (design D2). Remove `Effects::pushes`, `spawn_forge_work`, `FORGE_TIMEOUT`, `pr::on_push_report` and `pr::ensure_draft`. Verify with `cargo test -p df-agent-channel session` (one job per qualifying push, none for 0 commits ahead, none when a PR is recorded, diverged flag set in the batch).
- [x] 3.2 Write failing tests, then add `task_branch_pushed` to `RunEnd` (from `WorkspaceState`; `false` in `RunEnd::failure`) and enqueue in `end_run_tx` per design D2. Remove `pr::on_run_end`. Verify with `cargo test -p df-agent-channel runs`: completed and pushed → `open` and `ready`; completed without push → `ready` only (scenario "Completed without a push"); pushed but failed → `open` only; cancelled task or failure end without push → no job.
- [x] 3.3 Write a failing test, then enqueue `pr.sync { ready }` without `run_id` in `mark_task_done` when the task has a PR, and remove `mark_pr_ready` and `mark_ready_bounded` from `crates/df-web/src/task_pages.rs` with their tests. Verify with `cargo test -p df-db tasks` (job with PR, none without) and `cargo test -p df-web task_pages`.

## 4. Wiring

- [x] 4.1 Move the outbox handler list and dispatcher start from `crates/dark-factory/src/lifecycle_events.rs` to `crates/dark-factory/src/outbox.rs` and register `PrSyncHandler` (design D6). Verify with the moved kinds test asserting `[pr.mark_draft, pr.sync]` without and `[…, lifecycle_event.deliver]` with lifecycle events, via `cargo test -p dark-factory outbox`.
- [x] 4.2 Write an integration test in `crates/df-agent-channel` that runs a completed run end against a forge that fails "mark ready" twice, drives the dispatcher, and asserts the PR ends ready with the summary (scenario "Forge down at completion"); and one that enqueues at run end, drops the session before dispatch, starts a new dispatcher and asserts the same (scenario "Restart at completion"). Verify with `cargo test -p df-agent-channel`.

## 5. Docs and tech debt

- [x] 5.1 Update `docs/content/operations/troubleshooting.md` ("No pull request appears": no push ahead of base, or the forge plugin failed; retried with backoff for an hour, see `df.outbox.jobs{kind="pr.sync"}` and the control plane log; add a row "Pull request stays a draft after completion" with the same cause), and list the `pr.mark_draft` and `pr.sync` kinds at `df.outbox.jobs` in `docs/content/observability/telemetry.md`. Verify with `just docs-build`.
- [x] 5.2 Delete TD-11, TD-12, TD-13, TD-82, TD-91 and TD-108 (entries, summary rows, origin lines that name only them) from `docs/tech_debt.md`. Verify with `grep -n "TD-11\b\|TD-12\b\|TD-13\b\|TD-82\|TD-91\|TD-108" docs/tech_debt.md` returning nothing.

## 6. Integration checks

- [x] 6.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length` and `just bazel-test`. Verify all pass; list container-backed tests that could not run.

## Workflow follow-up

- On archive, add the backlog row "Drop `tasks.completion_summary`" (status *decided*, contract step of design D4, reference this change) to `product/iterations/roadmap.md`.
