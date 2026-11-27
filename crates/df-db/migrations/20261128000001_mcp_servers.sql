-- MCP servers (iter4-mcp-gateway design D2): upstream HTTP MCP servers with the one credential
-- Dark Factory uses for each, which projects enable them, and the calls their runs made.

CREATE TABLE mcp_servers (
    id                     UUID PRIMARY KEY,
    -- Immutable: it is part of every tool name agents see (`<name>__<tool>`).
    name                   TEXT NOT NULL UNIQUE,
    -- With path and query; not secret.
    url                    TEXT NOT NULL,
    credential_ref_id      UUID NOT NULL REFERENCES credential_refs(id) ON DELETE RESTRICT,
    credential_scheme      TEXT NOT NULL,
    -- Header name and value prefix of the `header` scheme.
    credential_header      TEXT,
    credential_prefix      TEXT,
    -- Parameter name of the `query` scheme.
    credential_query_param TEXT,
    -- Skip the gateway's private-address check.
    private_network        BOOLEAN NOT NULL DEFAULT false,
    -- Increases on every edit; gateways end their upstream sessions when it changes.
    config_version         BIGINT NOT NULL DEFAULT 1,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mcp_servers_scheme_check CHECK (
        (credential_scheme = 'bearer'
            AND credential_header IS NULL AND credential_prefix IS NULL
            AND credential_query_param IS NULL)
        OR (credential_scheme = 'header'
            AND credential_header IS NOT NULL AND credential_header <> ''
            AND credential_query_param IS NULL)
        OR (credential_scheme = 'query'
            AND credential_query_param IS NOT NULL AND credential_query_param <> ''
            AND credential_header IS NULL AND credential_prefix IS NULL)
    )
);

-- Enablement per project; nothing is enabled by default.
CREATE TABLE project_mcp_servers (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    server_id  UUID NOT NULL REFERENCES mcp_servers(id) ON DELETE RESTRICT,
    PRIMARY KEY (project_id, server_id)
);

CREATE INDEX project_mcp_servers_server_idx ON project_mcp_servers (server_id);

-- One row per `tools/call`, or per server left out of a `tools/list`. `server_name` is no foreign
-- key, so records survive the deletion of a server. The key makes a retried report idempotent.
CREATE TABLE mcp_calls (
    call_id        UUID PRIMARY KEY,
    run_id         UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    server_name    TEXT NOT NULL,
    tool           TEXT NOT NULL,
    outcome        TEXT NOT NULL,
    started_at     TIMESTAMPTZ NOT NULL,
    duration_ms    BIGINT NOT NULL,
    request_bytes  BIGINT NOT NULL,
    response_bytes BIGINT NOT NULL,
    CONSTRAINT mcp_calls_outcome_check CHECK (outcome IN (
        'ok', 'tool-error', 'rejected', 'upstream-error', 'timeout', 'too-large', 'cancelled'
    ))
);

CREATE INDEX mcp_calls_run_idx ON mcp_calls (run_id, started_at);
