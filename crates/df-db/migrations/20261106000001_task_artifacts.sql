-- Artifacts (iter3-artifacts): named text and image files that belong to one task. The content
-- lives in the row, so replacing an artifact leaves nothing to collect (design D1).
CREATE TABLE task_artifacts (
    task_id    UUID        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    name       TEXT        NOT NULL,
    kind       TEXT        NOT NULL,
    mime_type  TEXT        NOT NULL,
    size       BIGINT      NOT NULL,
    width      INT,
    height     INT,
    content    BYTEA       NOT NULL,
    -- Hex SHA-256 of `content`, the `ETag` of the image endpoint.
    sha256     TEXT        NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (task_id, name),
    CONSTRAINT task_artifacts_name_check CHECK (name ~ '^[A-Za-z0-9_-][A-Za-z0-9._-]{0,99}$'),
    CONSTRAINT task_artifacts_kind_check CHECK (kind IN ('text', 'image'))
);
