-- Durable desired-state queue for Gmail's \Seen and \Flagged flags. A single
-- row coalesces repeated read/star changes for a thread before IMAP sync.
CREATE TABLE IF NOT EXISTS mail_flag_operations (
    id             TEXT PRIMARY KEY,
    account_id     TEXT NOT NULL,
    thread_id      TEXT NOT NULL UNIQUE REFERENCES threads(id),
    seen           INTEGER,
    starred        INTEGER,
    status         TEXT NOT NULL DEFAULT 'pending'
                   CHECK (status IN ('pending', 'in_progress', 'failed')),
    attempt_count  INTEGER NOT NULL DEFAULT 0,
    next_retry_at  TEXT NOT NULL,
    last_error     TEXT,
    revision       INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mail_flag_operations_pending
    ON mail_flag_operations(status, next_retry_at);
