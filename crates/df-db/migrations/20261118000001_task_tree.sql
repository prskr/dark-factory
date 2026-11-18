-- Task tree (iter3-task-tree design D1): the base task of a task and the project's subtask merge
-- policy. Existing tasks get no base task, existing projects keep merges with humans.
ALTER TABLE tasks ADD COLUMN base_task_id UUID REFERENCES tasks(id);
CREATE INDEX tasks_base_task_id_idx ON tasks (base_task_id) WHERE base_task_id IS NOT NULL;

ALTER TABLE projects
    ADD COLUMN subtask_merge TEXT NOT NULL DEFAULT 'human'
        CHECK (subtask_merge IN ('human', 'parent-agent'));
