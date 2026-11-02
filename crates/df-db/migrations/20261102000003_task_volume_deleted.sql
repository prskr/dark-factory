-- Task volume lifecycle (iter2-task-volume-lifecycle): set when the task's PVC was deleted,
-- cleared when the next run creates it again.
ALTER TABLE tasks ADD COLUMN volume_deleted_at TIMESTAMPTZ;
