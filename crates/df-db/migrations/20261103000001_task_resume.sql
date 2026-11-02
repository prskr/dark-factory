-- Automatic resume (iter2-run-resilience): when the scheduler may start the task's next run, and
-- how many consecutive automatic resumes after infrastructure failures the task has had.
ALTER TABLE tasks ADD COLUMN resume_at TIMESTAMPTZ;
ALTER TABLE tasks ADD COLUMN auto_resumes INTEGER NOT NULL DEFAULT 0 CHECK (auto_resumes >= 0);
