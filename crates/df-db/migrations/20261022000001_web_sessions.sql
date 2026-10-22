-- Server-side sessions of the web UI (`df-web`): tokens and roles stay on the server.
CREATE TABLE web_sessions (
    id         TEXT PRIMARY KEY,
    data       JSONB NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX web_sessions_expires_at_idx ON web_sessions (expires_at);
