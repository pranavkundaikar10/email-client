CREATE TABLE IF NOT EXISTS email_analysis (
    thread_id     TEXT PRIMARY KEY REFERENCES threads(id),
    is_actionable INTEGER NOT NULL DEFAULT 0,
    importance    INTEGER NOT NULL DEFAULT 3,
    category      TEXT NOT NULL DEFAULT 'other',
    summary       TEXT NOT NULL DEFAULT '',
    action_items  TEXT NOT NULL DEFAULT '[]',
    deadline      TEXT,
    model         TEXT NOT NULL DEFAULT '',
    analyzed_at   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_email_analysis_importance ON email_analysis(importance DESC);
