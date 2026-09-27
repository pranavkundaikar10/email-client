CREATE TABLE IF NOT EXISTS follow_ups (
    thread_id   TEXT PRIMARY KEY REFERENCES threads(id),
    due_at      TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'completed')),
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_follow_ups_active_due
ON follow_ups(status, due_at ASC);
