-- Persona signing key (openspec change persona-commit-signing): the public half of the key a
-- persona's commits are signed with, shown in the UI and sent to the gateways. The private key
-- lives only in a Kubernetes Secret. Both columns are NULL until the key exists.
ALTER TABLE personas
    ADD COLUMN signing_public_key TEXT,
    ADD COLUMN signing_fingerprint TEXT,
    ADD CONSTRAINT personas_signing_key_complete CHECK (
        (signing_public_key IS NULL) = (signing_fingerprint IS NULL)
    );
