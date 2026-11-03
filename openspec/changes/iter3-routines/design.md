# Design

## Context

- **Task creation**: `df_db::tasks::create_task` (`crates/df-db/src/tasks.rs`) does the following in one transaction it opens itself:
  - allocates the key under the `projects.next_task_number` row lock;
  - inserts the task;
  - queues the `task.assigned` prompt;
  - publishes `task_changed` and `task_message_queued`.
  `iter3-task-dependencies` turns `NewTask.created_by` into a `Principal`.
- **Principal** (`crates/df-core/src/domain.rs`): `Human`, `Agent`, `System`, serialized as JSONB in `task_messages.author` and, after `iter3-task-dependencies`, in `tasks.created_by`.
- **Scheduler** (`crates/df-agent-channel/src/scheduler.rs`):
  - runs on every replica, with a `tick` every 5 s;
  - each tick reprovisions stale runs, starts queued tasks with `FOR UPDATE SKIP LOCKED` and enforces timeouts.
- **Config UI** (`crates/df-web/src/config_pages.rs`, 1,357 lines):
  - list pages with modal forms, using htmx;
  - live field feedback through a GET fragment (`/projects/forge`, `hx-trigger="keyup changed delay:300ms"`);
  - larger features live in submodules (`config_pages/push_policy.rs`).
- **Audit**: `df_db::audit::write_audit_entry` with `NewAuditEntry::with_details`, written in the caller's transaction.
- **Time**: `chrono` is a workspace dependency. There is no timezone database in the dependency graph, and the static musl images ship no `/usr/share/zoneinfo`.

## Goals / Non-Goals

**Goals:**
- Firing goes through the same `create_task` path as a board member's task, so start, webhooks and delegation need no routine-specific code.
- One transaction per firing: claim, overlap check, task creation and outcome either all happen or none do.

**Non-Goals:**
- A separate scheduler process or leader election. The existing tick on every replica suffices.
- Second-level precision. A firing happens within one tick (5 s) of its due time.

## Decisions

### D1: Table `routines`

```sql
CREATE TABLE routines (
  id            UUID PRIMARY KEY,
  project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name          TEXT NOT NULL,
  persona_id    UUID NOT NULL REFERENCES personas(id),
  prompt        TEXT NOT NULL CHECK (prompt <> ''),
  schedule      TEXT NOT NULL,
  timezone      TEXT NOT NULL DEFAULT 'UTC',
  enabled       BOOLEAN NOT NULL DEFAULT TRUE,
  next_fire_at  TIMESTAMPTZ,          -- NULL while disabled
  last_fired_at TIMESTAMPTZ,
  last_outcome  TEXT CHECK (last_outcome IN ('created', 'skipped')),
  last_task_id  UUID REFERENCES tasks(id) ON DELETE SET NULL,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (project_id, name)
);
CREATE INDEX routines_due ON routines (next_fire_at) WHERE enabled;
```

`last_task_id` does two jobs:
- After `created`, it is the new task.
- After `skipped`, it is still the open task that caused the skip, because nothing was created.

So the overlap check and the outcome display both read the same column. No second key column is needed.

`next_fire_at` is recomputed from now on create, on enable, and on every edit of `schedule` or `timezone`. It is set to NULL on disable.

Alternative: a `routine_firings` history table. Rejected: the decision was "no history table".

### D2: `Schedule` in `df-core`

`Schedule::parse(expr, tz) -> Result<Schedule, ScheduleError>` and `Schedule::next_after(DateTime<Utc>) -> Option<DateTime<Utc>>`:
- `expr` must split into exactly five whitespace-separated fields before it reaches `croner`. That rejects a seconds field and aliases such as `@daily`, independent of `croner`'s defaults.
- `croner` (latest release, `chrono` feature) parses the expression. Its extensions (`L`, `W`, `#`) are accepted as they come.
- `tz` is parsed as a `chrono_tz::Tz`. `next_after` computes the next occurrence in that zone and converts it to UTC, so 02:00 stays 02:00 local across DST changes.
- `ScheduleError` has two variants: invalid schedule and unknown timezone. The form maps each to its field.

Alternatives:
- `croner` with the `jiff` feature, which has bundled tzdb. `jiff` is only a transitive dependency through `gix-date`, while `chrono` is used across the workspace. Mixing two time libraries in one domain type is worse than adding `chrono-tz`.
- System tzdata: not present in the static images.

### D3: Principal `Routine`

`Principal::Routine { routine_id: RoutineId, name: String }`. `name` is copied at firing time. It is used only as `tasks.created_by`. The assignment prompt stays authored by the system principal (`task.assigned`), as for any task.

### D4: Firing in the scheduler tick

`tick` gets a fourth step, `fire_due_routines(now)`, before `start_in_background`, so a created task can start in the same tick. It runs one transaction per routine, in batches of 10:

1. `SELECT … FROM routines WHERE enabled AND next_fire_at <= $now ORDER BY next_fire_at LIMIT 1 FOR UPDATE SKIP LOCKED`. Another replica skips the locked row and can't fire it twice. Once committed, `next_fire_at` is in the future, so the row is no longer due.
2. Overlap check: `SELECT status FROM tasks WHERE id = last_task_id`. The routine is open unless the status is in `in_review`, `completed`, `failed` or `cancelled`, or `last_task_id` is NULL. The status set is the same constant as the open-subtask check of `iter3-task-tree`. Whichever change lands first adds it to `df-core`.
3. If not open: `create_task_tx(&mut tx, NewTask { … })`, a variant of `create_task` that takes the caller's transaction (`create_task` becomes a thin wrapper). Inputs:
   - title `format!("{name} {date}")`, with the date of `now` in the routine's timezone;
   - description is the prompt;
   - `created_by` is `Principal::Routine`;
   - no parent.
4. `UPDATE routines SET next_fire_at = schedule.next_after(now), last_fired_at = now, last_outcome = …, last_task_id = …`.
5. Commit, then emit an `info!` with `routine_id`, `project`, `outcome` and `task`.

"Next after now", not "next after the old `next_fire_at`", gives at most one catch-up firing after downtime. A routine whose schedule can't be parsed any more, which only happens if validation rules tighten later, gets `enabled = false` and a `warn!` instead of being retried every tick.

Alternative: `pg_cron` or a Kubernetes CronJob. Rejected: an external dependency, and ADR-0003 already picked `croner` in the control plane.

### D5: Run now

`run_routine_now(pool, routine_id, actor)` uses the same body as step 2–3 of D4, with these differences:
- It locks with plain `FOR UPDATE`. It waits for a concurrent tick, then sees its outcome.
- It updates `last_fired_at`, `last_outcome` and `last_task_id`, but not `next_fire_at`.
- It writes the `routine.run_now` audit entry with the outcome in the same transaction.

D4 and D5 share one function, `fire_locked(tx, routine, now) -> Outcome`. They differ only in the lock clause and in whether `next_fire_at` moves.

### D6: CRUD and audit

`df_db::routines`: `list(project filter)`, `get`, `create`, `update`, `delete` and `run_now`. Each write runs in one transaction with its `routine.*` audit entry, and `before`/`after` are the routine's fields without the `last_*` and `next_fire_at` columns. Validation (name, prompt, `Schedule::parse`) happens in `df-web` before the call. The unique constraint on `(project_id, name)` maps to a field error at the name.

### D7: UI

- **Module and routes**: a new module `config_pages/routines.rs` (`config_pages.rs` is near the 1,500-line limit). Routes:
  - `/routines` (list, `?project=`)
  - `/routines/new`
  - `/routines/{id}/edit`
  - `POST /routines`
  - `POST /routines/{id}`
  - `POST /routines/{id}/delete` (with a confirm dialog)
  - `POST /routines/{id}/run`
  - `GET /routines/preview?schedule=&timezone=`
- **Form**: fields project (select, fixed after creation), name, persona (select), prompt (textarea), schedule (text, placeholder `0 2 * * *`), timezone (text with a `datalist` of `chrono_tz::TZ_VARIANTS`, default `UTC`) and enabled.
- **Next-firing preview**: uses the `/projects/forge` pattern. Schedule and timezone trigger `hx-get` to the preview fragment, which shows either the next firing in the routine's timezone or the field error.
- **List row**: name, project key, persona, schedule and timezone, an enabled toggle, next firing, last firing, last outcome with the task link, Run now, Edit and Delete. Run now and the toggle re-render the row.
- **Rail**: the Routines icon goes below Projects.
- **Task details**: for `Principal::Routine` they show "Routine · \<name\>". The link to `/routines?project=<key>` is shown only while a routine with that id exists, found by one lookup when the page is rendered.

## Risks / Trade-offs

- [A 5 s tick delays a firing by up to 5 s] → acceptable for cron granularity. The tick already runs on every replica.
- [A routine set to `* * * * *` creates a task every minute when its tasks end quickly] → the overlap rule caps it at one open task. Board members are trusted, and the cost shows in usage.
- [DST gaps and repeats: `0 2 * * *` on a spring-forward day has no 02:00, and in autumn 02:30 occurs twice] → `croner`'s handling applies (OCPS: a skipped time fires at the next valid instant, a repeated time fires once). It is covered by a `df-core` test, and the routines reference documents it.
- [`chrono-tz` embeds the tzdb at build time, so zone rule changes need a release] → Renovate keeps `chrono-tz` current. This is acceptable for a scheduling feature.
- [Two changes add the "open" status set, this one and `iter3-task-tree`] → one constant in `df-core`, added by whichever lands first.
- [`web-ui-shell` rail delta conflicts with `iter3-chats`, which adds Chats to the same requirement] → whichever of the two is archived second rebases its MODIFIED block on the archived text (tasks 6.1).

## Migration Plan

The change is additive: a new table, a new principal variant and new routes. Rollback is a down migration that drops `routines`. Tasks that already have a `Routine` creator principal would fail to deserialize on an older binary, which is acceptable before the MVP.
