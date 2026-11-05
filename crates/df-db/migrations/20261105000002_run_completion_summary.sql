-- Summary passed to `complete_task`, per run: the run's end posts only its own summary on the
-- pull request. `tasks.completion_summary` stays unused until a later change drops it
-- (expand/contract, ADR-0012); values are not copied.
ALTER TABLE runs ADD COLUMN completion_summary TEXT NOT NULL DEFAULT '';
