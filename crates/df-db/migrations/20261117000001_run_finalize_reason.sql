-- Why a run is `finalizing` (ADR-0061): the session picks `Release` (`idle`) or `Cancel` (the
-- others) from it. Written with `finalizing` by the run applier, never cleared.
ALTER TABLE runs ADD COLUMN finalize_reason TEXT
    CONSTRAINT runs_finalize_reason_check CHECK (
        finalize_reason IS NULL OR finalize_reason IN ('idle', 'budget', 'stop', 'done')
    );

-- A run that is `finalizing` now: a stop request decides, else the idle release. The reason of an
-- earlier budget or done is not known; the hard stop still ends such a run.
UPDATE runs
SET finalize_reason = CASE WHEN stop_requested_at IS NOT NULL THEN 'stop' ELSE 'idle' END
WHERE state = 'finalizing';
