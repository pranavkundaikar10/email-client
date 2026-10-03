-- Durable local ownership for the one-at-a-time background AI worker.
-- This never represents a Gmail action; it only prevents duplicate local
-- inference and lets a later app session recover work interrupted by sleep or
-- a crash.
CREATE TABLE IF NOT EXISTS background_triage_jobs (
    thread_id          TEXT PRIMARY KEY REFERENCES threads(id),
    message_id         TEXT NOT NULL REFERENCES messages(id),
    message_received_at TEXT NOT NULL,
    status             TEXT NOT NULL DEFAULT 'pending'
                       CHECK (status IN ('pending', 'leased')),
    attempt_count      INTEGER NOT NULL DEFAULT 0,
    next_attempt_at    TEXT NOT NULL,
    lease_expires_at   TEXT,
    last_error         TEXT,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_background_triage_jobs_ready
    ON background_triage_jobs(status, next_attempt_at, message_received_at DESC);
