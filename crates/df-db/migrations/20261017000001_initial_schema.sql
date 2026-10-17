-- Initial foundation schema for Dark Factory (design D4).
-- Tables use unqualified names so they are created in the connection's active
-- search_path (`dark_factory` in production and `migrate`, or per-test database in `#[sqlx::test]`).

CREATE TABLE users (
    id           UUID PRIMARY KEY,
    issuer       TEXT NOT NULL,
    subject      TEXT NOT NULL,
    display_name TEXT NOT NULL,
    first_seen   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT users_issuer_subject_unique UNIQUE (issuer, subject)
);

CREATE TABLE credential_refs (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    secret_name TEXT NOT NULL,
    secret_key  TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE projects (
    id                  UUID PRIMARY KEY,
    key                 TEXT NOT NULL UNIQUE,
    name                TEXT NOT NULL,
    repo_url            TEXT NOT NULL,
    issue_tracker_url   TEXT,
    image               TEXT NOT NULL,
    base_branch         TEXT NOT NULL DEFAULT 'main',
    forge_credential_id UUID REFERENCES credential_refs(id),
    next_task_number    BIGINT NOT NULL DEFAULT 1,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT projects_key_check CHECK (key ~ '^[A-Z][A-Z0-9]{1,9}$'),
    CONSTRAINT projects_next_task_number_check CHECK (next_task_number >= 1)
);

CREATE TABLE personas (
    id                    UUID PRIMARY KEY,
    name                  TEXT NOT NULL UNIQUE,
    system_prompt         TEXT NOT NULL,
    harness               TEXT NOT NULL DEFAULT 'claude-code',
    adapter_command       TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
    model                 TEXT,
    harness_credential_id UUID REFERENCES credential_refs(id),
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT personas_harness_check CHECK (harness IN ('claude-code'))
);

CREATE TABLE tasks (
    id                 UUID PRIMARY KEY,
    project_id         UUID NOT NULL REFERENCES projects(id),
    number             BIGINT NOT NULL,
    key                TEXT NOT NULL UNIQUE,
    title              TEXT NOT NULL,
    description        TEXT NOT NULL DEFAULT '',
    persona_id         UUID NOT NULL REFERENCES personas(id),
    issue_number       BIGINT,
    status             TEXT NOT NULL DEFAULT 'queued',
    status_line        TEXT NOT NULL DEFAULT '',
    pr_number          BIGINT,
    pr_url             TEXT,
    pr_title           TEXT,
    pr_body            TEXT,
    diverged           BOOLEAN NOT NULL DEFAULT FALSE,
    harness_session_id TEXT NOT NULL DEFAULT '',
    created_by         UUID REFERENCES users(id),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT tasks_project_number_unique UNIQUE (project_id, number),
    CONSTRAINT tasks_number_check CHECK (number >= 1),
    CONSTRAINT tasks_key_check CHECK (key ~ '^[A-Z][A-Z0-9]{1,9}-[1-9][0-9]*$'),
    CONSTRAINT tasks_status_check CHECK (
        status IN (
            'queued',
            'running',
            'waiting',
            'completed',
            'rate_limited',
            'limit_reached',
            'failed'
        )
    )
);

CREATE TABLE runs (
    id                 UUID PRIMARY KEY,
    task_id            UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    number             BIGINT NOT NULL,
    state              TEXT NOT NULL DEFAULT 'pending',
    outcome            TEXT,
    detail             TEXT NOT NULL DEFAULT '',
    retry_after        TIMESTAMPTZ,
    pod_ip              TEXT,
    last_persisted_seq BIGINT NOT NULL DEFAULT 0,
    task_branch_sha    TEXT NOT NULL DEFAULT '',
    wip_sha            TEXT NOT NULL DEFAULT '',
    harness_session_id TEXT NOT NULL DEFAULT '',
    started_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at           TIMESTAMPTZ,
    CONSTRAINT runs_task_number_unique UNIQUE (task_id, number),
    CONSTRAINT runs_number_check CHECK (number >= 1),
    CONSTRAINT runs_state_check CHECK (
        state IN (
            'pending',
            'provisioning',
            'connecting',
            'preparing',
            'running',
            'awaiting_input',
            'finalizing',
            'succeeded',
            'blocked',
            'failed',
            'cancelled'
        )
    ),
    CONSTRAINT runs_outcome_check CHECK (
        outcome IS NULL OR outcome IN (
            'completed',
            'end_turn',
            'blocked',
            'limit_reached',
            'rate_limited',
            'cancelled',
            'failed',
            'provisioning_failed',
            'connect_timeout',
            'sandbox_lost'
        )
    )
);

-- At most one active (non-terminal) run per task.
CREATE UNIQUE INDEX runs_one_active_per_task_idx
    ON runs (task_id)
    WHERE state NOT IN ('succeeded', 'blocked', 'failed', 'cancelled');

CREATE TABLE task_messages (
    id               UUID PRIMARY KEY,
    task_id          UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author           JSONB NOT NULL,
    content          JSONB NOT NULL,
    queued_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_run_id UUID REFERENCES runs(id),
    delivered_at     TIMESTAMPTZ
);

CREATE INDEX task_messages_task_undelivered_idx
    ON task_messages (task_id, queued_at)
    WHERE delivered_at IS NULL;

CREATE TABLE transcript_events (
    id             BIGSERIAL PRIMARY KEY,
    task_id        UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    run_id         UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    seq            BIGINT NOT NULL,
    schema_version INTEGER NOT NULL DEFAULT 1,
    occurred_at    TIMESTAMPTZ NOT NULL,
    kind           TEXT NOT NULL,
    payload        JSONB COMPRESSION lz4 NOT NULL,
    CONSTRAINT transcript_events_run_seq_unique UNIQUE (run_id, seq),
    CONSTRAINT transcript_events_seq_check CHECK (seq >= 1)
);

CREATE INDEX transcript_events_task_id_idx ON transcript_events (task_id, id);

CREATE TABLE tool_calls (
    call_id      UUID PRIMARY KEY,
    run_id       UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    tool_name    TEXT NOT NULL,
    arguments    JSONB NOT NULL,
    result       JSONB,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ
);

CREATE TABLE egress_allowlist (
    id         UUID PRIMARY KEY,
    scope      TEXT NOT NULL,
    project_id UUID REFERENCES projects(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,
    host       TEXT,
    port       INTEGER,
    namespace  TEXT,
    service    TEXT,
    ports      INTEGER[] NOT NULL DEFAULT '{}'::INTEGER[],
    allow_dns  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT egress_allowlist_scope_check CHECK (
        (scope = 'instance' AND project_id IS NULL)
        OR (scope = 'project' AND project_id IS NOT NULL)
    ),
    CONSTRAINT egress_allowlist_kind_check CHECK (
        (kind = 'http-host' AND host IS NOT NULL AND port IS NOT NULL)
        OR (kind = 'cluster-service' AND namespace IS NOT NULL AND service IS NOT NULL)
    )
);

CREATE SEQUENCE egress_policy_version_seq START WITH 1 INCREMENT BY 1;

CREATE TABLE blobs (
    sha256     TEXT PRIMARY KEY,
    mime_type  TEXT NOT NULL,
    size       BIGINT NOT NULL,
    data       BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT blobs_sha256_check CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT blobs_size_check CHECK (size >= 0)
);

CREATE TABLE audit_log (
    id            BIGSERIAL PRIMARY KEY,
    occurred_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    actor         JSONB NOT NULL,
    roles         TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
    action        TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id   TEXT NOT NULL,
    details       JSONB NOT NULL DEFAULT '{}'::JSONB
);

CREATE INDEX audit_log_occurred_at_idx ON audit_log (occurred_at);
