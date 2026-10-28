-- Agent requests (questions and typed approvals, ADR-0038) and the task status `blocked`: the
-- last run waited for an answer or approval that did not arrive within the idle deadline.
-- Run state `awaiting_input` already exists in `runs_state_check`. A run that waited for an
-- answer ends `succeeded` with outcome `blocked`; the run state `blocked` is gone.
-- Dev databases from before this change may hold blocked runs.
UPDATE runs SET state = 'succeeded', outcome = COALESCE(outcome, 'blocked')
WHERE state = 'blocked';
ALTER TABLE runs DROP CONSTRAINT runs_state_check;
ALTER TABLE runs ADD CONSTRAINT runs_state_check CHECK (
    state IN (
        'pending',
        'provisioning',
        'connecting',
        'preparing',
        'running',
        'awaiting_input',
        'finalizing',
        'succeeded',
        'failed',
        'cancelled'
    )
);
DROP INDEX runs_one_active_per_task_idx;
CREATE UNIQUE INDEX runs_one_active_per_task_idx
    ON runs (task_id)
    WHERE state NOT IN ('succeeded', 'failed', 'cancelled');
ALTER TABLE tasks DROP CONSTRAINT tasks_status_check;
ALTER TABLE tasks ADD CONSTRAINT tasks_status_check CHECK (
    status IN (
        'queued',
        'running',
        'waiting',
        'blocked',
        'in_review',
        'completed',
        'rate_limited',
        'limit_reached',
        'failed'
    )
);

CREATE TABLE agent_requests (
    id            UUID PRIMARY KEY,
    task_id       UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- Null when the control plane raised the request.
    run_id        UUID REFERENCES runs(id) ON DELETE SET NULL,
    kind          TEXT NOT NULL,
    -- Null for questions.
    approval_type TEXT,
    -- Question: {text, suggested_answers}; approval: defined by the type.
    payload       JSONB NOT NULL,
    reason        TEXT NOT NULL DEFAULT '',
    blocking      BOOLEAN NOT NULL DEFAULT false,
    fingerprint   TEXT NOT NULL,
    state         TEXT NOT NULL DEFAULT 'pending',
    -- {answer}, {scope} or {comment}.
    decision      JSONB,
    decided_by    JSONB,
    decided_at    TIMESTAMPTZ,
    close_reason  TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT agent_requests_kind_check CHECK (kind IN ('question', 'approval')),
    CONSTRAINT agent_requests_state_check CHECK (
        state IN ('pending', 'answered', 'approved', 'denied', 'closed')
    ),
    CONSTRAINT agent_requests_kind_type_check CHECK (
        (kind = 'question' AND approval_type IS NULL)
        OR (kind = 'approval' AND approval_type IS NOT NULL)
    )
);

-- Identical pending requests of a task are merged.
CREATE UNIQUE INDEX agent_requests_pending_unique_idx
    ON agent_requests (task_id, kind, (COALESCE(approval_type, '')), fingerprint)
    WHERE state = 'pending';

CREATE INDEX agent_requests_pending_idx ON agent_requests (created_at DESC) WHERE state = 'pending';
CREATE INDEX agent_requests_task_idx ON agent_requests (task_id, created_at);
