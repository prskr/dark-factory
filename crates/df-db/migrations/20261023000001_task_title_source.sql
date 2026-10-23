-- Where a task's title came from; ranks user > generated > agent > placeholder.
-- Existing tasks were titled by a person.
ALTER TABLE tasks ADD COLUMN title_source TEXT NOT NULL DEFAULT 'user';
ALTER TABLE tasks ALTER COLUMN title_source DROP DEFAULT;
ALTER TABLE tasks ADD CONSTRAINT tasks_title_source_check
    CHECK (title_source IN ('placeholder', 'agent', 'generated', 'user'));
