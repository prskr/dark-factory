-- Persona identity (openspec change persona-identity): handle, display name and commit email.
-- The old unique name becomes the display name; existing personas get a handle derived from it.

ALTER TABLE personas RENAME COLUMN name TO display_name;
ALTER TABLE personas DROP CONSTRAINT personas_name_key;
ALTER TABLE personas ADD COLUMN handle TEXT, ADD COLUMN commit_email TEXT;

DO $$
DECLARE
    r RECORD;
    base TEXT;
    candidate TEXT;
    suffix TEXT;
    n INT;
    name TEXT;
BEGIN
    FOR r IN SELECT id, display_name FROM personas ORDER BY created_at, id LOOP
        base := left(
            btrim(regexp_replace(lower(r.display_name), '[^a-z0-9]+', '-', 'g'), '-'),
            39
        );
        IF base = '' THEN
            base := 'persona';
        END IF;
        candidate := base;
        n := 1;
        -- Checked against every handle assigned so far, not per slug group.
        WHILE EXISTS (SELECT 1 FROM personas WHERE handle = candidate) LOOP
            n := n + 1;
            suffix := '-' || n;
            candidate := rtrim(left(base, 39 - length(suffix)), '-') || suffix;
        END LOOP;

        name := regexp_replace(r.display_name, '[\x01-\x1f\x7f-\x9f<>]', '', 'g');
        name := left(regexp_replace(name, '^\s+|\s+$', '', 'g'), 100);
        IF name = '' THEN
            name := candidate;
        END IF;

        UPDATE personas
        SET handle = candidate,
            display_name = name,
            commit_email = candidate || '@dark-factory.localhost'
        WHERE id = r.id;
    END LOOP;
END $$;

ALTER TABLE personas
    ALTER COLUMN handle SET NOT NULL,
    ALTER COLUMN commit_email SET NOT NULL,
    ADD CONSTRAINT personas_handle_key UNIQUE (handle),
    ADD CONSTRAINT personas_commit_email_key UNIQUE (commit_email),
    ADD CONSTRAINT personas_handle_format CHECK (handle ~ '^[a-z0-9][a-z0-9-]{0,38}$'),
    ADD CONSTRAINT personas_commit_email_format CHECK (
        commit_email ~ '^[^@<>,[:space:][:cntrl:]]+@[^@<>,[:space:][:cntrl:]]+$'
        AND length(commit_email) <= 254
    );

-- The identity a run commits with, frozen when the run is created: an identity edit applies to
-- later runs only. The trigger fills it from the task's persona, so every way of creating a run
-- freezes it.
ALTER TABLE runs ADD COLUMN commit_name TEXT, ADD COLUMN commit_email TEXT;

UPDATE runs r
SET commit_name = p.display_name, commit_email = p.commit_email
FROM tasks t
JOIN personas p ON p.id = t.persona_id
WHERE t.id = r.task_id;

ALTER TABLE runs
    ALTER COLUMN commit_name SET NOT NULL,
    ALTER COLUMN commit_email SET NOT NULL;

CREATE FUNCTION runs_freeze_commit_identity() RETURNS trigger AS $$
BEGIN
    IF NEW.commit_name IS NULL OR NEW.commit_email IS NULL THEN
        SELECT p.display_name, p.commit_email INTO NEW.commit_name, NEW.commit_email
        FROM tasks t JOIN personas p ON p.id = t.persona_id
        WHERE t.id = NEW.task_id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER runs_freeze_commit_identity
    BEFORE INSERT ON runs
    FOR EACH ROW EXECUTE FUNCTION runs_freeze_commit_identity();
