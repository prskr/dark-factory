-- Task scope for allowlist entries (iter2-egress-approvals design D1): exactly one of instance,
-- project or task scope; task scope only for `http-host`.
ALTER TABLE egress_allowlist
    ADD COLUMN task_id UUID REFERENCES tasks(id) ON DELETE CASCADE;

ALTER TABLE egress_allowlist DROP CONSTRAINT egress_allowlist_scope_check;
ALTER TABLE egress_allowlist ADD CONSTRAINT egress_allowlist_scope_check CHECK (
    (scope = 'instance' AND project_id IS NULL AND task_id IS NULL)
    OR (scope = 'project' AND project_id IS NOT NULL AND task_id IS NULL)
    OR (scope = 'task' AND task_id IS NOT NULL AND project_id IS NULL AND kind = 'http-host')
);

CREATE INDEX egress_allowlist_task_idx ON egress_allowlist (task_id) WHERE task_id IS NOT NULL;
