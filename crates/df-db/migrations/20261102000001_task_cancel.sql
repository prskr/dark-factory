-- Cancelling a task (iter2-cancel): task status `cancelled`, the stop marker of a run and the
-- discarded state of undelivered messages.
ALTER TABLE tasks DROP CONSTRAINT tasks_status_check;
ALTER TABLE tasks ADD CONSTRAINT tasks_status_check CHECK (
    status IN (
        'queued',
        'running',
        'waiting',
        'blocked',
        'in_review',
        'completed',
        'cancelled',
        'rate_limited',
        'limit_reached',
        'failed'
    )
);

-- Set when a board member stops the run; drives the run-end mapping and the stop timeout.
ALTER TABLE runs ADD COLUMN stop_requested_at TIMESTAMPTZ;

-- Set when cancelling the task dropped the message before any run received it.
ALTER TABLE task_messages ADD COLUMN discarded_at TIMESTAMPTZ;
