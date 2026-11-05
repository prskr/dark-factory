-- `queued_at` becomes the time of the insert instead of the start of the transaction. A message
-- inserts after it took the task row lock, so its time is ordered like the lock: a message that
-- waited for the lock of a run end or "mark done" is queued after their completion time
-- (`pr-forge-outbox` design D3).
ALTER TABLE task_messages ALTER COLUMN queued_at SET DEFAULT clock_timestamp();
