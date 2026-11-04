-- Model providers (iter3-inference-gateway design D5): an upstream model service with its wire
-- APIs and the one credential Dark Factory uses for it. Personas reference exactly one provider
-- instead of a harness credential.

CREATE TABLE model_providers (
    id                UUID PRIMARY KEY,
    name              TEXT NOT NULL UNIQUE,
    credential_ref_id UUID NOT NULL REFERENCES credential_refs(id),
    credential_scheme TEXT NOT NULL,
    -- Header name and value prefix of the `header` scheme.
    credential_header TEXT,
    credential_prefix TEXT,
    -- Extra headers: [{"name": ..., "template": ...}].
    headers           JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT model_providers_scheme_check
        CHECK (credential_scheme IN ('x-api-key', 'bearer', 'header', 'claude-subscription'))
);

CREATE TABLE model_provider_apis (
    provider_id  UUID NOT NULL REFERENCES model_providers(id) ON DELETE CASCADE,
    wire_api     TEXT NOT NULL,
    upstream_url TEXT NOT NULL,
    PRIMARY KEY (provider_id, wire_api),
    CONSTRAINT model_provider_apis_wire_api_check
        CHECK (wire_api IN ('anthropic-messages', 'openai-chat', 'openai-responses'))
);

ALTER TABLE personas
    ADD COLUMN model_provider_id UUID REFERENCES model_providers(id) ON DELETE RESTRICT;

-- Personas without a harness credential cannot get a provider.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM personas WHERE harness_credential_id IS NULL) THEN
        RAISE EXCEPTION
            'personas without a harness credential cannot be migrated to a model provider: %',
            (SELECT string_agg(name, ', ' ORDER BY name) FROM personas WHERE harness_credential_id IS NULL)
            USING HINT = 'Assign a harness credential to these personas (or delete them) and run the migration again.';
    END IF;
END $$;

-- One Claude subscription provider per credential the personas use today.
INSERT INTO model_providers (id, name, credential_ref_id, credential_scheme)
SELECT gen_random_uuid(), 'Claude subscription (' || c.name || ')', c.id, 'claude-subscription'
FROM credential_refs c
WHERE c.id IN (SELECT harness_credential_id FROM personas);

INSERT INTO model_provider_apis (provider_id, wire_api, upstream_url)
SELECT id, 'anthropic-messages', 'https://api.anthropic.com' FROM model_providers;

UPDATE personas p
SET model_provider_id = m.id
FROM model_providers m
WHERE m.credential_ref_id = p.harness_credential_id;

ALTER TABLE personas ALTER COLUMN model_provider_id SET NOT NULL;
ALTER TABLE personas DROP COLUMN harness_credential_id;
