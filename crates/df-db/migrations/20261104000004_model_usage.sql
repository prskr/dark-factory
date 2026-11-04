-- Token usage per model request as measured by the gateway (iter3-inference-gateway design D5).
-- `request_id` is chosen by the gateway, so a retried report inserts nothing twice. A request
-- whose response carried no usage has `usage_known = false` and zero counts.

CREATE TABLE model_usage (
    request_id      UUID PRIMARY KEY,
    run_id          UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    recorded_at     TIMESTAMPTZ NOT NULL,
    model           TEXT,
    wire_api        TEXT NOT NULL,
    upstream_status INTEGER,
    input_tokens    BIGINT NOT NULL DEFAULT 0,
    output_tokens   BIGINT NOT NULL DEFAULT 0,
    cache_read_tokens  BIGINT NOT NULL DEFAULT 0,
    cache_write_tokens BIGINT NOT NULL DEFAULT 0,
    usage_known     BOOLEAN NOT NULL,
    CONSTRAINT model_usage_wire_api_check
        CHECK (wire_api IN ('anthropic-messages', 'openai-chat', 'openai-responses'))
);

CREATE INDEX model_usage_run_idx ON model_usage (run_id);
