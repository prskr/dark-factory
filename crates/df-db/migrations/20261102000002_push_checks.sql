-- Push checks of the Git proxy (iter2-push-checks design D4, D5).

-- A project's push policy. A project without a row uses the defaults of the spec.
CREATE TABLE project_push_policies (
    project_id      UUID PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    -- `globset` syntax, validated when saved.
    protected_globs TEXT[] NOT NULL,
    protected_mode  TEXT NOT NULL,
    secret_mode     TEXT NOT NULL,
    size_mode       TEXT NOT NULL,
    max_blob_bytes  BIGINT NOT NULL,
    linear_history  BOOLEAN NOT NULL DEFAULT false,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT project_push_policies_mode_check CHECK (
        protected_mode IN ('reject', 'approval')
        AND secret_mode IN ('reject', 'approval')
        AND size_mode IN ('reject', 'approval')
    ),
    CONSTRAINT project_push_policies_max_blob_bytes_check CHECK (max_blob_bytes > 0)
);

-- Approved push check findings of a task; a push passes findings with a listed fingerprint.
CREATE TABLE push_check_exceptions (
    task_id     UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    fingerprint TEXT NOT NULL,
    -- The approved `push-check-exception` request.
    request_id  UUID NOT NULL REFERENCES agent_requests(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (task_id, fingerprint)
);

-- `push.rejected` messages queued since the task's last accepted task-branch push.
ALTER TABLE tasks ADD COLUMN push_rejections_since_accept INTEGER NOT NULL DEFAULT 0;
