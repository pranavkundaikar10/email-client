-- Keep the durable queue finite: repeated model failures should be visible as
-- failed work, not consume local inference forever.
CREATE TABLE background_triage_jobs_new (
    thread_id           TEXT PRIMARY KEY REFERENCES threads(id),
    message_id          TEXT NOT NULL REFERENCES messages(id),
    message_received_at TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending', 'leased', 'failed')),
    attempt_count       INTEGER NOT NULL DEFAULT 0,
    next_attempt_at     TEXT NOT NULL,
    lease_expires_at    TEXT,
    last_error          TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);
INSERT INTO background_triage_jobs_new
SELECT thread_id, message_id, message_received_at, status, attempt_count,
       next_attempt_at, lease_expires_at, last_error, created_at, updated_at
FROM background_triage_jobs;
DROP TABLE background_triage_jobs;
ALTER TABLE background_triage_jobs_new RENAME TO background_triage_jobs;
CREATE INDEX idx_background_triage_jobs_ready
    ON background_triage_jobs(status, next_attempt_at, message_received_at DESC);
