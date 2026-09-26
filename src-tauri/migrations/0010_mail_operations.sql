-- Remote Gmail changes are first recorded locally so an app restart or a
-- transient IMAP failure cannot lose the user's intent.
CREATE TABLE IF NOT EXISTS mail_operations (
    id             TEXT PRIMARY KEY,
    account_id     TEXT NOT NULL,
    thread_id      TEXT NOT NULL UNIQUE REFERENCES threads(id),
    operation      TEXT NOT NULL CHECK (operation IN ('archive', 'trash')),
    status         TEXT NOT NULL DEFAULT 'pending'
                   CHECK (status IN ('pending', 'in_progress', 'failed')),
    attempt_count  INTEGER NOT NULL DEFAULT 0,
    next_retry_at  TEXT NOT NULL,
    last_error     TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mail_operations_pending
    ON mail_operations(status, next_retry_at);
