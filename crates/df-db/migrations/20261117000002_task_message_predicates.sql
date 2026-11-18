-- The two message predicates, defined once. `undelivered`: the prompt still waits for a run.
-- `held_back`: the same, not yet forwarded, so a human may still edit or withdraw it.
ALTER TABLE task_messages
    ADD COLUMN undelivered boolean
        GENERATED ALWAYS AS (delivered_at IS NULL AND discarded_at IS NULL) STORED,
    ADD COLUMN held_back boolean
        GENERATED ALWAYS AS (
            delivered_at IS NULL AND forwarded_at IS NULL AND discarded_at IS NULL
        ) STORED;

-- The old index also covered discarded rows.
DROP INDEX task_messages_task_undelivered_idx;
CREATE INDEX task_messages_task_undelivered_idx
    ON task_messages (task_id, queued_at)
    WHERE undelivered;
