-- When a queued message was handed to the run's runtime. Held-back messages (NULL) stay editable.
-- Messages undelivered at upgrade time may already have been forwarded by the old control plane,
-- so they count as forwarded.
ALTER TABLE task_messages ADD COLUMN forwarded_at TIMESTAMPTZ;
UPDATE task_messages SET forwarded_at = now() WHERE delivered_at IS NULL;
