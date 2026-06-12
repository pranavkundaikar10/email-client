CREATE TABLE IF NOT EXISTS accounts (
    id          TEXT PRIMARY KEY,
    email       TEXT NOT NULL UNIQUE,
    provider    TEXT NOT NULL,
    synced_at   TEXT
);

CREATE TABLE IF NOT EXISTS threads (
    id          TEXT PRIMARY KEY,
    account_id  TEXT NOT NULL REFERENCES accounts(id),
    subject     TEXT NOT NULL DEFAULT '',
    snippet     TEXT NOT NULL DEFAULT '',
    unread      INTEGER NOT NULL DEFAULT 1,
    starred     INTEGER NOT NULL DEFAULT 0,
    archived    INTEGER NOT NULL DEFAULT 0,
    snoozed_until TEXT,
    last_message_at TEXT NOT NULL,
    label_ids   TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE IF NOT EXISTS messages (
    id          TEXT PRIMARY KEY,
    thread_id   TEXT NOT NULL REFERENCES threads(id),
    account_id  TEXT NOT NULL REFERENCES accounts(id),
    from_email  TEXT NOT NULL,
    from_name   TEXT NOT NULL DEFAULT '',
    to_emails   TEXT NOT NULL DEFAULT '[]',
    cc_emails   TEXT NOT NULL DEFAULT '[]',
    subject     TEXT NOT NULL DEFAULT '',
    body_html   TEXT,
    body_text   TEXT,
    sent_at     TEXT NOT NULL,
    unread      INTEGER NOT NULL DEFAULT 1,
    body_fetched INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_threads_account_last ON threads(account_id, last_message_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_thread ON messages(thread_id, sent_at DESC);

CREATE VIRTUAL TABLE IF NOT EXISTS threads_fts USING fts5(
    subject, snippet, from_email,
    content='threads',
    content_rowid='rowid'
);
