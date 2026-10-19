-- Automatic catch-up is intentionally time-bounded, while a user-requested
-- historical batch must remain eligible regardless of age. Existing jobs were
-- created by the automatic worker, so they safely default to `automatic`.
ALTER TABLE background_triage_jobs
    ADD COLUMN source TEXT NOT NULL DEFAULT 'automatic'
    CHECK (source IN ('automatic', 'manual'));

CREATE INDEX IF NOT EXISTS idx_background_triage_jobs_source_status
    ON background_triage_jobs(source, status, message_received_at DESC);
