-- Forge webhooks (iter3-webhooks).

-- Whose reviews and mentions wake the project's tasks.
ALTER TABLE projects ADD COLUMN webhook_wake TEXT NOT NULL DEFAULT 'members'
    CHECK (webhook_wake IN ('none', 'members', 'anyone'));

-- Set by the first merge event of the task's pull request.
ALTER TABLE tasks ADD COLUMN pr_merged_at TIMESTAMPTZ;
