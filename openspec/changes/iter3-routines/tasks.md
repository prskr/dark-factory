# Tasks

Prerequisite, merged before this change: `iter3-task-dependencies` (`tasks.created_by` as a `Principal`, the "Creator and parent in the task details" requirement). Its `task-ui` spec must be archived so that the MODIFIED delta here applies.

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Schedule and principal (`df-core`)

- [ ] 1.1 Add `croner` (latest release, `default-features = false`, feature `chrono`) and `chrono-tz` (latest release) to `[workspace.dependencies]` and to `df-core`, then update `Cargo.lock` and the Bazel lock files (skill `rust`). Verify with `cargo build -p df-core` and `cargo deny check`.
- [ ] 1.2 Write failing tests, then implement `Schedule::parse(expr, tz)` and `Schedule::next_after` (design D2). The tests check that:
  - `0 2 * * *` / `Europe/Berlin` after 2026-11-03T10:00Z gives 2026-11-04T01:00Z;
  - the DST-end case from the spec gives 00:00Z on 2026-10-24 and 01:00Z on 2026-10-26;
  - a spring-forward day (2026-03-29, `30 2 * * *`) fires once, at the next valid instant;
  - `0 0 2 * * *`, `@daily` and `every night` are invalid schedules;
  - `Europe/Atlantis` is an unknown timezone.

  Verify with `cargo test -p df-core schedule`.
- [ ] 1.3 Write a failing serde round-trip test, then add `RoutineId` and `Principal::Routine { routine_id, name }` (design D3), and add the shared "open task" status set if `iter3-task-tree` hasn't yet. Verify with `cargo test -p df-core principal` and `cargo clippy --all-targets -- -D warnings`.

## 2. Storage (`df-db`)

- [ ] 2.1 Add the `routines` migration (design D1) and regenerate `.sqlx/`. Verify with a `df-db` migration test (a duplicate `(project_id, name)` is rejected, an empty prompt is rejected) and with `cargo build --workspace` offline.
- [ ] 2.2 Write failing tests, then extract `create_task_tx(&mut tx, NewTask)` from `create_task`, which becomes a wrapper. Verify with the existing `df-db` task tests unchanged, plus one test that creates a task inside a rolled-back transaction and finds no task, no message and no key gap.
- [ ] 2.3 Write failing tests, then implement `df_db::routines` CRUD with `routine.create`, `routine.update` and `routine.delete` audit entries in the same transaction (design D6). `next_fire_at` is recomputed on create, enable, and schedule or timezone edits, and is NULL while disabled. Verify with `df-db` tests:
  - before/after values in the audit entries;
  - a schedule edit at 01:00 from `0 2 * * *` to `0 3 * * *` gives 03:00 the same day;
  - delete leaves the created tasks unchanged;
  - list with and without a project filter.
- [ ] 2.4 Write failing tests, then implement `fire_locked` and `fire_due(pool, now, batch)` (design D4). Verify with `df-db` tests:
  - a due routine creates a queued task "Nightly deps 2026-11-04" with persona, prompt, no parent and `Principal::Routine`, and moves `next_fire_at`;
  - a routine whose last task is `running` is skipped, keeps `last_task_id` and gets outcome `skipped`;
  - a last task in `in_review` doesn't block;
  - a disabled routine isn't fired;
  - after downtime across three hourly times, one task is created and the next firing is the next full hour;
  - two concurrent `fire_due` calls create exactly one task.
- [ ] 2.5 Write failing tests, then implement `run_now(pool, routine_id, actor)` (design D5) with the `routine.run_now` audit entry. Verify with `df-db` tests:
  - a disabled routine creates a task and stays disabled with `next_fire_at` NULL;
  - an enabled routine keeps its `next_fire_at`;
  - an open last task gives `skipped` and an audit entry with that outcome.

## 3. Scheduler (`df-agent-channel`)

- [ ] 3.1 Write a failing test, then call `fire_due` from `Scheduler::tick` before `start_in_background`, and log each outcome at info level with `routine_id`, `project`, `outcome` and `task`. A routine whose stored schedule no longer parses gets `enabled = false` and a `warn!`. Verify with a `df-agent-channel` DB test: one `tick` with a due routine creates the task and starts its run in the same tick, and the log capture (`df-telemetry` `test-util`) has the event.

## 4. Web UI (`df-web`)

- [ ] 4.1 Draw the routines page (list rows, modal form with next-firing preview, phone layout) in Penpot first (`docs/design-workflow.md`). Verify that the design is reviewed and linked in the PR.
- [ ] 4.2 Write failing tests, then add `config_pages/routines.rs` with the list page, `?project=` filter, empty state, and the modal create/edit form with validation errors at the fields (design D7; specs `routines` "Routines page", `configuration-management`). Verify with `df-web` tests:
  - create, a duplicate name error, an invalid schedule keeping the modal values;
  - the filtered list;
  - the empty state text.
- [ ] 4.3 Write failing tests, then add `GET /routines/preview` and wire it to the schedule and timezone fields with `hx-get` and `keyup changed delay:300ms`. Verify with `df-web` tests: a valid input renders the next firing in the routine's timezone, and an invalid one renders the field error.
- [ ] 4.4 Write failing tests, then add the enabled toggle, Run now, and delete with confirm. Each re-renders the row. Verify with `df-web` tests:
  - the outcome cell shows "created DF-n" or "skipped: DF-n still open" with the key linked;
  - next firing is empty while disabled;
  - delete removes the row.
- [ ] 4.5 Write failing tests, then add Routines to the navigation rail below Projects (not in the phone bottom bar), and show "Routine · \<name\>" as creator in the task details, linked only while the routine exists (spec `task-ui`). Verify with `df-web` tests for the rail on `/routines`, a routine-created task, and the same task after the routine is deleted.

## 5. Documentation

- [ ] 5.1 Write the routines reference in `docs/routines.md`. Verify that every field and behavior in the spec `routines` is covered. Content:
  - the fields with defaults;
  - the five-field cron syntax and timezone, and the DST behavior from design Risks;
  - the overlap rule and what "open" means;
  - catch-up after downtime;
  - Run now;
  - the outcome column;
  - audit entries;
  - a Mermaid flowchart of a firing (due → open check → create or skip → next time).
- [ ] 5.2 Write the how-to "Check stale Renovate PRs with a CEO routine" in `docs/routines.md`. Verify that the steps match the UI from group 4 and the prerequisites are named. Content:
  - prerequisite `iter3-branch-workflows`;
  - routine setup;
  - an example prompt;
  - what happens on a night when the previous sweep is still running.

## 6. Integration

- [ ] 6.1 If `iter3-chats` was archived before this change, rebase the `web-ui-shell` "Desktop navigation rail" MODIFIED block on the archived text (Chats, Tasks, Projects, Routines, Personas, Credentials). Verify with `openspec validate iter3-routines --strict`.
- [ ] 6.2 Extend the e2e suite (`crates/df-e2e`). Verify with `just e2e-local`. The test:
  - creates a routine in the UI;
  - presses Run now;
  - checks that the task "\<name\> \<today\>" starts with the fake ACP agent and shows "Routine · \<name\>" as creator;
  - presses Run now again while the task is running and sees "skipped".
- [ ] 6.3 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length` and the Bazel build (`bazel --bazelrc=.bazelrc.agents build //...`). Verify that all pass.

## Workflow follow-up

- Archive the change after review; then update `product/iterations/iteration-3.md` if the scope changed during implementation.
