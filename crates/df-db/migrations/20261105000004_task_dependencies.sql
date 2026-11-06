-- Task dependencies and delegation (iter3-task-dependencies): parent and creator of a task,
-- dependencies on other tasks of the project, the status `awaiting_dependencies` and the persona
-- description.

-- The task a task reports to; set from the calling run's task when an agent creates a task.
ALTER TABLE tasks ADD COLUMN parent_id UUID REFERENCES tasks(id);
CREATE INDEX tasks_parent_id_idx ON tasks (parent_id) WHERE parent_id IS NOT NULL;

-- `created_by` becomes a principal, the shape of `task_messages.author`.
ALTER TABLE tasks ADD COLUMN created_by_principal JSONB;
UPDATE tasks
SET created_by_principal = CASE
    WHEN created_by IS NULL THEN '{"kind": "system", "reason": "migration"}'::jsonb
    ELSE jsonb_build_object(
        'kind', 'human',
        'user_id', created_by,
        'display_name', COALESCE((SELECT display_name FROM users WHERE users.id = tasks.created_by), '')
    )
END;
ALTER TABLE tasks DROP COLUMN created_by;
ALTER TABLE tasks RENAME COLUMN created_by_principal TO created_by;
ALTER TABLE tasks ALTER COLUMN created_by SET NOT NULL;

-- The first time the task completed (the agent called `complete_task`) or became done. Each is
-- set once; a dependency is satisfied by these times alone. Existing tasks get the times their
-- status implies.
ALTER TABLE tasks ADD COLUMN first_completed_at TIMESTAMPTZ;
ALTER TABLE tasks ADD COLUMN first_done_at TIMESTAMPTZ;
UPDATE tasks SET first_completed_at = updated_at WHERE status IN ('in_review', 'completed');
UPDATE tasks SET first_done_at = updated_at WHERE status = 'completed';

CREATE TABLE task_dependencies (
    task_id            UUID NOT NULL REFERENCES tasks(id),
    depends_on_task_id UUID NOT NULL REFERENCES tasks(id),
    kind               TEXT NOT NULL,
    PRIMARY KEY (task_id, depends_on_task_id),
    CONSTRAINT task_dependencies_kind_check CHECK (kind IN ('completion', 'done')),
    CONSTRAINT task_dependencies_not_self CHECK (task_id <> depends_on_task_id)
);
CREATE INDEX task_dependencies_target_idx ON task_dependencies (depends_on_task_id);

ALTER TABLE tasks DROP CONSTRAINT tasks_status_check;
ALTER TABLE tasks ADD CONSTRAINT tasks_status_check CHECK (
    status IN (
        'awaiting_dependencies',
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

-- Role and strengths of a persona, read by agents through `list_personas`.
ALTER TABLE personas ADD COLUMN description TEXT NOT NULL DEFAULT '';
ALTER TABLE personas ADD CONSTRAINT personas_description_length
    CHECK (char_length(description) <= 500);
