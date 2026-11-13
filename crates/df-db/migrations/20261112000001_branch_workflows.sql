-- Branch workflows (iter3-branch-workflows design D1): the project's rewrite policy, the stored
-- task branch with its "one open task per branch" rule, the base of a task attached to a PR, and
-- the attached-PR flag, and a trigger that defaults `branch`. Existing projects get `with-lease`;
-- existing tasks keep `factory/task/<key>`.
ALTER TABLE projects
    ADD COLUMN branch_rewrite_policy TEXT NOT NULL DEFAULT 'with-lease'
        CHECK (branch_rewrite_policy IN ('never', 'with-lease'));

ALTER TABLE tasks ADD COLUMN branch TEXT;
UPDATE tasks SET branch = 'factory/task/' || key;
ALTER TABLE tasks ALTER COLUMN branch SET NOT NULL;
-- Only for a task attached to a PR into a base other than the project's.
ALTER TABLE tasks ADD COLUMN base_branch TEXT;
ALTER TABLE tasks ADD COLUMN pr_attached BOOLEAN NOT NULL DEFAULT false;

CREATE UNIQUE INDEX tasks_open_branch ON tasks (project_id, branch)
    WHERE status NOT IN ('completed', 'failed', 'cancelled');

-- A task inserted without a branch commits to its own `factory/task/<key>`.
CREATE FUNCTION tasks_default_branch() RETURNS trigger AS $$
BEGIN
    NEW.branch := 'factory/task/' || NEW.key;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER tasks_default_branch BEFORE INSERT ON tasks
    FOR EACH ROW WHEN (NEW.branch IS NULL) EXECUTE FUNCTION tasks_default_branch();
