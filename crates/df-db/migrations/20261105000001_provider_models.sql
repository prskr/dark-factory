-- Models on the model provider (iter3-opencode-harness design D4): a persona selects one model
-- of its provider; the model carries the wire API and the optional token limits.

CREATE TABLE model_provider_models (
    provider_id           UUID NOT NULL REFERENCES model_providers(id) ON DELETE CASCADE,
    model_id              TEXT NOT NULL,
    wire_api              TEXT NOT NULL,
    context_window_tokens BIGINT,
    max_output_tokens     BIGINT,
    PRIMARY KEY (provider_id, model_id),
    -- A model only uses an API its provider offers; removing the API is refused.
    FOREIGN KEY (provider_id, wire_api) REFERENCES model_provider_apis (provider_id, wire_api),
    CONSTRAINT model_provider_models_limits_check CHECK (
        (context_window_tokens IS NULL AND max_output_tokens IS NULL)
        OR (context_window_tokens > 0 AND max_output_tokens > 0
            AND context_window_tokens IS NOT NULL AND max_output_tokens IS NOT NULL)
    )
);

-- Personas without a model cannot get one (pre-MVP).
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM personas WHERE model IS NULL OR btrim(model) = '') THEN
        RAISE EXCEPTION
            'personas without a model cannot be migrated to a provider model: %',
            (SELECT string_agg(name, ', ' ORDER BY name) FROM personas WHERE model IS NULL OR btrim(model) = '')
            USING HINT = 'Set a model on these personas (or delete them) and run the migration again.';
    END IF;
END $$;

-- Every model a persona uses today is a model of its provider on Anthropic Messages, the only API
-- existing personas use.
INSERT INTO model_provider_models (provider_id, model_id, wire_api)
SELECT DISTINCT model_provider_id, model, 'anthropic-messages' FROM personas;

ALTER TABLE personas ALTER COLUMN model SET NOT NULL;
ALTER TABLE personas
    ADD CONSTRAINT personas_model_fkey FOREIGN KEY (model_provider_id, model)
    REFERENCES model_provider_models (provider_id, model_id) ON DELETE RESTRICT;

ALTER TABLE personas DROP CONSTRAINT personas_harness_check;
ALTER TABLE personas
    ADD CONSTRAINT personas_harness_check CHECK (harness IN ('claude-code', 'opencode'));
