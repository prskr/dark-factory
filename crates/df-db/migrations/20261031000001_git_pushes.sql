-- One row per push through the gateway's Git proxy (iter2-git-proxy design D6).
CREATE TABLE git_pushes (
    push_id     UUID PRIMARY KEY,
    run_id      UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    gateway_pod TEXT NOT NULL,
    -- [{name, old_sha, new_sha, verdict, reason}] in command order.
    refs        JSONB NOT NULL DEFAULT '[]'::jsonb,
    verdict     TEXT NOT NULL,
    bytes       BIGINT NOT NULL DEFAULT 0,
    duration_ms BIGINT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT git_pushes_verdict_check
        CHECK (verdict IN ('forwarding', 'accepted', 'rejected', 'upstream-failed', 'unknown'))
);

CREATE INDEX git_pushes_verdict_created_idx ON git_pushes (verdict, created_at);
CREATE INDEX git_pushes_run_idx ON git_pushes (run_id);
