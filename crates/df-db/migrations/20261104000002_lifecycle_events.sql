-- Lifecycle events (lifecycle-events): a generic transactional outbox, the recorded events and
-- the project subscriptions that receive them.

-- One job per row. A dispatcher on the leader claims due jobs by kind and runs the handler.
CREATE TABLE outbox (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind            TEXT NOT NULL,
    payload         JSONB NOT NULL,
    attempts        INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_until     TIMESTAMPTZ,
    done_at         TIMESTAMPTZ,
    gave_up_at      TIMESTAMPTZ,
    last_error      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX outbox_pending_idx ON outbox (next_attempt_at)
    WHERE done_at IS NULL AND gave_up_at IS NULL;

-- The full event envelope, built once when the state change commits. A snapshot: later renames
-- do not change it, and deleting the project does not remove it (cleanup removes unreferenced
-- events).
CREATE TABLE lifecycle_events (
    id          UUID PRIMARY KEY,
    type        TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    project_id  UUID NOT NULL,
    payload     JSONB NOT NULL
);

CREATE TABLE event_subscriptions (
    id              UUID PRIMARY KEY,
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    url             TEXT NOT NULL,
    -- Event types to send; empty means all, including types added later.
    events          TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_attempt_at TIMESTAMPTZ,
    last_status     SMALLINT,
    last_error      TEXT
);

CREATE INDEX event_subscriptions_project_idx ON event_subscriptions (project_id);

-- Set when the task's volume was created for the first time since it last did not exist; the
-- update that sets it emits `volume.provisioned`. Reset when the volume is deleted.
ALTER TABLE tasks ADD COLUMN volume_provisioned_at TIMESTAMPTZ;

-- Tasks whose volume already exists emit no late `volume.provisioned`.
UPDATE tasks SET volume_provisioned_at = now()
WHERE volume_deleted_at IS NULL AND EXISTS (SELECT 1 FROM runs WHERE runs.task_id = tasks.id);

-- When the scheduler picked the run (`sandbox.provisioning`) and when its pod came up
-- (`sandbox.provisioned`).
ALTER TABLE runs ADD COLUMN provisioning_started_at TIMESTAMPTZ;
ALTER TABLE runs ADD COLUMN sandbox_started_at TIMESTAMPTZ;
