# Proposal

## Why

The tasks a board member cares about are split across two chips. "Active" shows only queued, running and rate-limited tasks; every task that waits for input, is blocked, is in review or hit a limit is under "Waiting". Checking open work means switching chips all the time.

## What Changes

- **"Active" covers every task that is not done:** queued, running, rate limited, awaiting dependencies, waiting, blocked, in review and limit reached. It stays the default filter.
- **The "Waiting" chip is removed.** The chips are Active, Done and All.
- **`?status=waiting` stays as a link-only filter**, like `queued` and `running`: no chip, all chips unselected. The project counters ("Queued / Running / Waiting / Done") keep linking to it.
- The empty-list text for Active names the new set.
- **BREAKING (UI):** bookmarks of `?status=active` now show more tasks. Pre-MVP, no compatibility needed.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `task-ui`: "Task list filters". Active covers every non-final status, there is no Waiting chip, and `waiting` becomes a link-only filter.
- `task-management`: "Task status groups". The task list filters no longer use the "Waiting" group as a chip; the group stays for the project counters and the `waiting` link filter.

## Impact

- `df-core` (`domain.rs`): `StatusSet::Active` holds every non-final status. `StatusGroup` (badges, counters) is unchanged.
- `df-web` (`task_pages/view.rs`): `StatusFilter::CHIPS` without `Waiting`, `Waiting` kept as a link-only value, new empty-list text. Tests in `task_pages/tests/list.rs`.
- `df-db`: none; the list query already takes the set's statuses.
- Docs: `docs/content/using/tasks.md` (list filters).
- No new dependencies, no migration.

## Out of Scope

- Changing the project counters' columns: rejected, they keep Queued, Running, Waiting and Done.
- A separate "Needs me" filter (waiting, blocked) apart from Active: rejected.
