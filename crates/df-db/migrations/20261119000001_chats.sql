-- Chats (iter3-chats design D1): a chat is a task of kind `chat` with the key
-- `<PROJECT>-<n>-CHAT`. Existing rows are tasks. Only a chat is archived; archiving keeps the
-- status.
ALTER TABLE tasks
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'task'
        CONSTRAINT tasks_kind_check CHECK (kind IN ('task', 'chat')),
    ADD COLUMN archived_at TIMESTAMPTZ,
    ADD COLUMN archived_by JSONB;

ALTER TABLE tasks
    ADD CONSTRAINT tasks_archived_check CHECK (
        (archived_at IS NULL) = (archived_by IS NULL)
        AND (kind = 'chat' OR archived_at IS NULL)
    );

ALTER TABLE tasks DROP CONSTRAINT tasks_key_check;
ALTER TABLE tasks
    ADD CONSTRAINT tasks_key_check CHECK (key ~ '^[A-Z][A-Z0-9]{1,9}-[1-9][0-9]*(-CHAT)?$'),
    ADD CONSTRAINT tasks_chat_key_check CHECK ((kind = 'chat') = (key LIKE '%-CHAT'));

-- Since when a run has waited for the next message (`awaiting_input`); the scheduler releases
-- the idle chat run that waited longest when a queued chat finds no free slot.
ALTER TABLE runs ADD COLUMN idle_since TIMESTAMPTZ;

-- A chat has no branch (stored as the empty string): the "one open task per branch" rule skips it.
DROP INDEX tasks_open_branch;
CREATE UNIQUE INDEX tasks_open_branch ON tasks (project_id, branch)
    WHERE branch <> '' AND status NOT IN ('completed', 'failed', 'cancelled');

CREATE INDEX tasks_chat_list_idx ON tasks (project_id, kind, archived_at);

-- The last activity of a chat: its creation, its latest transcript event or its latest message.
CREATE FUNCTION task_last_activity(task_id UUID, created_at TIMESTAMPTZ) RETURNS TIMESTAMPTZ
    LANGUAGE sql STABLE AS $$
    SELECT GREATEST(created_at,
                    (SELECT max(e.occurred_at) FROM transcript_events e WHERE e.task_id = $1),
                    (SELECT max(m.queued_at) FROM task_messages m WHERE m.task_id = $1))
$$;
