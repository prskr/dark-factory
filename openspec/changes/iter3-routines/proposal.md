# Proposal

## Why

Recurring work, such as a nightly dependency check or a CEO sweeping stale Renovate PRs, needs a board member to create the same task by hand every time. ADR-0016 lists routines as a run trigger and ADR-0003 picked `croner` for schedules, but nothing creates tasks on a schedule yet. Iteration 3's demo step 6 needs a routine that creates a nightly task.

## What Changes

- **Routine**: a new configuration entity per project with:
  - `name`, unique per project;
  - persona and prompt;
  - `schedule`, a 5-field cron expression without seconds;
  - IANA `timezone`, default `UTC`;
  - `enabled`.
- **Firing**:
  - Each firing creates a new ordinary task in the routine's project: kind `task`, no parent, the routine's persona, and the prompt as description.
  - The title is `<name> <local date of the firing>`, for example "Nightly deps 2026-11-03".
  - The task's creator is a new principal `Routine { routine_id, name }`. The name is copied, so it survives a rename or delete.
- **Overlap**: a firing is skipped while the routine's last task is open. Open means any status except `in_review`, `completed`, `failed` and `cancelled`, the same definition as an open subtask in `iter3-task-tree`.
- **Scheduling**:
  - The scheduler tick fires every enabled routine whose next firing time has passed, exactly once across replicas.
  - It then computes the next time from now. After downtime a routine fires at most once to catch up.
- **Outcome**: each routine stores its last firing time and last outcome: created `<KEY>`, or skipped because `<KEY>` is still open. There is no history table, and skips are also logged.
- **UI**:
  - A `/routines` page in the navigation rail, listing all routines with a project filter and showing each routine's next firing, last firing and last outcome.
  - A modal form that validates the cron expression and timezone and previews the next firing.
  - Enable and disable.
  - **Run now**. It also works on a disabled routine and follows the overlap rule.
  - Hard delete.
  - Board members only. Agents get no routine tools.
- **Task details**: a task created by a routine shows "Routine · \<name\>" as its creator.
- **Audit**: `routine.create`, `routine.update`, `routine.delete` and `routine.run_now`, with before and after values.
- **Docs**:
  - a routines reference;
  - a how-to "Check stale Renovate PRs with a CEO routine", which needs `iter3-branch-workflows` and `iter3-ci-tools`;
  - glossary term Routine (written).

Out of scope:
- **Agent tools for routines**: instance-management operations for agents, iteration 6 ("Full permission model", `roadmap.md`).
- **Firing history**: rejected. Tasks created by a routine carry it as creator, and the audit log keeps configuration changes.
- **Minimum interval, seconds field, interval schedules** ("every 90 minutes"): rejected. The overlap rule prevents pile-ups.
- **Backfilling several missed firings**: rejected. At most one catch-up firing.
- **Title templates, `depends_on`, `base_task`, task-scoped egress rules per routine**: rejected until a use case shows up.
- **Wake-ups on an existing task (monitors, heartbeats)**: backlog (`roadmap.md`, Monitors; ADR-0016).

## Capabilities

### New Capabilities
- `routines`: the routine entity, its validation, firing and overlap rule, catch-up after downtime, Run now, outcome display, the `/routines` page and the audit entries.

### Modified Capabilities
- `task-ui` ("Creator and parent in the task details", added by `iter3-task-dependencies`): shows a routine as creator.
- `web-ui-shell` ("Desktop navigation rail"): adds Routines.
- `configuration-management` ("Configuration forms as modals", "Configuration pages on phones"): includes routines.

## Impact

- `crates/df-core`:
  - `Principal::Routine { routine_id, name }`;
  - `RoutineId`;
  - schedule validation and the next-firing computation.
- `crates/df-db`:
  - migration for `routines`;
  - CRUD with audit;
  - claiming due routines with `FOR UPDATE SKIP LOCKED`;
  - firing in one transaction with `create_task`;
  - `.sqlx/`.
- `crates/df-agent-channel`: the scheduler tick fires due routines.
- `crates/df-web`: `/routines` page, modal form, Run now, the rail entry and the creator in the task details.
- Dependencies:
  - `croner`, new, latest release, `chrono` feature;
  - `chrono-tz`, new, for IANA timezones with a compiled-in database, because the static musl images have no `/usr/share/zoneinfo`.
- `crates/df-e2e`: a routine is created and fired with Run now, and its task shows the routine as creator.
- Depends on `iter3-task-dependencies`, which stores `tasks.created_by` as a `Principal`.
