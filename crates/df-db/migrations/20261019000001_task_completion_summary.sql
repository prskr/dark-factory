-- Summary passed to `complete_task`; posted on the pull request when the run ends as completed.
ALTER TABLE tasks ADD COLUMN completion_summary TEXT NOT NULL DEFAULT '';

-- First moment the runtime of a run connected; the wall-clock budget counts from here.
ALTER TABLE runs ADD COLUMN connected_at TIMESTAMPTZ;
