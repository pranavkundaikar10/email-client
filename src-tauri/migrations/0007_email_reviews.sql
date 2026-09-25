CREATE TABLE IF NOT EXISTS email_reviews (
    thread_id   TEXT PRIMARY KEY REFERENCES threads(id),
    decision    TEXT NOT NULL CHECK (decision IN ('keep', 'follow_up', 'archived')),
    reviewed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_email_reviews_reviewed_at ON email_reviews(reviewed_at DESC);
