# Spec Delta

## Purpose

Routines create tasks on a schedule: a fixed persona and prompt in a project, fired by a cron expression in a timezone, so recurring work needs no human to start it.

## ADDED Requirements

### Requirement: Routine definition
Board members SHALL create, edit and delete routines. A routine SHALL have a project, a name unique within that project, a persona, a non-empty prompt, a schedule (a cron expression with exactly five fields: minute, hour, day of month, month, day of week), an IANA timezone (default `UTC`) and an enabled flag (default on). Deleting a routine SHALL NOT change tasks it created.

#### Scenario: Create a routine
- **WHEN** a board member saves routine "Nightly deps" in project `DF` with persona "CEO", a prompt, schedule `0 2 * * *` and timezone `Europe/Berlin`
- **THEN** the routine is stored enabled and appears on the routines page

#### Scenario: Duplicate name in a project
- **WHEN** project `DF` has routine "Nightly deps" and another routine with that name is saved in `DF`
- **THEN** the form shows a validation error at the name and nothing is stored

#### Scenario: Same name in another project
- **WHEN** project `OPS` gets a routine "Nightly deps" while `DF` has one
- **THEN** both are stored

#### Scenario: Deleted routine
- **WHEN** a board member deletes "Nightly deps", which created `DF-41`
- **THEN** the routine is gone and `DF-41` is unchanged

### Requirement: Schedule validation
Saving a routine SHALL fail with a validation error at the field when the schedule is not a valid five-field cron expression or the timezone is not a known IANA timezone. The form SHALL show the next firing time in the routine's timezone for a valid schedule and timezone before saving.

#### Scenario: Seconds field
- **WHEN** a routine with schedule `0 0 2 * * *` is saved
- **THEN** the form shows a validation error at the schedule and nothing is stored

#### Scenario: Unknown timezone
- **WHEN** a routine with timezone `Europe/Atlantis` is saved
- **THEN** the form shows a validation error at the timezone and nothing is stored

#### Scenario: Next firing preview
- **WHEN** a board member enters `0 2 * * *` and `Europe/Berlin` at 2026-11-03 10:00 UTC
- **THEN** the form shows 2026-11-04 02:00 Europe/Berlin as next firing

### Requirement: Firing creates a task
When an enabled routine is due, Dark Factory SHALL create a new task in the routine's project with the routine's persona, the prompt as description, no parent, the title `<name> <date>` (the date of the firing in the routine's timezone, `YYYY-MM-DD`) and the routine (with its name at firing time) as creator. The task SHALL start like any task created by a board member.

#### Scenario: Nightly firing
- **WHEN** routine "Nightly deps" (`0 2 * * *`, `Europe/Berlin`, persona "CEO") becomes due at 2026-11-04 02:00 Europe/Berlin
- **THEN** a queued task "Nightly deps 2026-11-04" with persona "CEO", the routine's prompt as description and no parent exists, created by routine "Nightly deps"

#### Scenario: Disabled routine
- **WHEN** a disabled routine's schedule time passes
- **THEN** no task is created

### Requirement: Firing respects daylight saving time
The schedule SHALL be evaluated in the routine's timezone, so a firing time stays the same local time across daylight saving changes.

#### Scenario: Clock change
- **WHEN** routine `0 2 * * *` in `Europe/Berlin` fires on 2026-10-24 and on 2026-10-26
- **THEN** both tasks are created at 02:00 local time (00:00 UTC and 01:00 UTC)

### Requirement: Skipped while the last task is open
A firing SHALL be skipped, without creating a task, while the last task the routine created is open: in any status except `in_review`, `completed`, `failed` and `cancelled`. The routine's next firing time SHALL advance as for a firing that created a task.

#### Scenario: Previous task still running
- **WHEN** "Nightly deps" becomes due while its last task `DF-37` is `running`
- **THEN** no task is created and the routine's last outcome is "skipped: DF-37 still open"

#### Scenario: Previous task in review
- **WHEN** "Nightly deps" becomes due while `DF-37` is `in_review`
- **THEN** a new task is created

#### Scenario: Previous task waiting on a subtask
- **WHEN** `DF-37` cannot complete because its subtask is still running, and "Nightly deps" becomes due
- **THEN** the firing is skipped

### Requirement: Exactly one firing per due time
A due routine SHALL fire once, even when several control plane replicas run. After a firing (created or skipped), the next firing time SHALL be the first schedule time after the moment of firing. When firing times passed while no replica was running, the routine SHALL fire once and not once per missed time.

#### Scenario: Two replicas
- **WHEN** two control plane replicas see "Nightly deps" due at the same time
- **THEN** exactly one task is created

#### Scenario: Downtime over several firings
- **WHEN** an hourly routine was due at 02:00, 03:00 and 04:00 while the control plane was down, and the control plane starts at 04:30
- **THEN** one task is created and the next firing is 05:00

#### Scenario: Schedule edited
- **WHEN** a board member changes the schedule of an enabled routine from `0 2 * * *` to `0 3 * * *` at 01:00
- **THEN** the next firing is 03:00 the same day

### Requirement: Run now
Board members SHALL be able to fire a routine immediately with Run now, also when it is disabled. Run now SHALL follow the overlap rule and record its outcome like a scheduled firing, and SHALL NOT change the next scheduled firing.

#### Scenario: Trying a disabled routine
- **WHEN** a board member presses Run now on disabled routine "Nightly deps" and it has no open task
- **THEN** a task "Nightly deps <today>" is created and the routine stays disabled

#### Scenario: Run now while the last task is open
- **WHEN** a board member presses Run now while the routine's last task `DF-37` is `running`
- **THEN** no task is created and the page shows "skipped: DF-37 still open"

### Requirement: Routines page
The routines page SHALL list all routines with name, project, persona, schedule with timezone, enabled state, next firing (none while disabled), last firing time and last outcome (a link to the created task, or "skipped" with the open task's key linked). It SHALL filter by project, as part of the URL. Create and edit SHALL use a modal form.

#### Scenario: Routine with a created task
- **WHEN** "Nightly deps" last fired at 02:00 and created `DF-41`
- **THEN** its row shows 02:00, "created DF-41" with `DF-41` linked, and the next firing

#### Scenario: Project filter
- **WHEN** `/routines?project=DF` is opened
- **THEN** only routines of project `DF` are listed

#### Scenario: No routines yet
- **WHEN** the routines page is opened and no routine exists
- **THEN** it explains in one sentence what a routine does and offers a New routine button

### Requirement: Skipped firings are logged
Every firing SHALL emit a log event with the routine, its project and the outcome (the created task's key, or the key of the open task that caused the skip).

#### Scenario: Skip logged
- **WHEN** a firing of "Nightly deps" is skipped because `DF-37` is open
- **THEN** an info-level event with the routine id, project `DF`, outcome `skipped` and `DF-37` is logged

### Requirement: Routine changes are audited
Creating, editing and deleting a routine and Run now SHALL write an audit entry (`routine.create`, `routine.update`, `routine.delete`, `routine.run_now`) with the acting board member and the before and after values.

#### Scenario: Prompt edited
- **WHEN** a board member changes the prompt of "Nightly deps"
- **THEN** an audit entry `routine.update` with the old and new prompt exists

#### Scenario: Run now audited
- **WHEN** a board member presses Run now on "Nightly deps"
- **THEN** an audit entry `routine.run_now` with the board member and the outcome exists

### Requirement: Routines are managed by humans only
Agents SHALL have no tool to create, change, delete or fire routines.

#### Scenario: Factory tool list
- **WHEN** an agent lists its factory tools
- **THEN** none of them creates, changes, deletes or fires a routine
