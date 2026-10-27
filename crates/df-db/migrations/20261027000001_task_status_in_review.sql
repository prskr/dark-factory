-- `in_review`: the agent called `complete_task`. `completed` now means a human marked the task done.
ALTER TABLE tasks DROP CONSTRAINT tasks_status_check;
ALTER TABLE tasks ADD CONSTRAINT tasks_status_check CHECK (
    status IN (
        'queued',
        'running',
        'waiting',
        'in_review',
        'completed',
        'rate_limited',
        'limit_reached',
        'failed'
    )
);
