-- Transcript events written by the control plane (forge activity, ADR-0046) have no runtime
-- sequence number; they are identified by a source key instead. Exactly one of the two is set.
ALTER TABLE transcript_events ALTER COLUMN seq DROP NOT NULL;
ALTER TABLE transcript_events ADD COLUMN source_key TEXT;
ALTER TABLE transcript_events ADD CONSTRAINT transcript_events_source_check
    CHECK (num_nonnulls(seq, source_key) = 1);
ALTER TABLE transcript_events ADD CONSTRAINT transcript_events_run_source_key_unique
    UNIQUE (run_id, source_key);
