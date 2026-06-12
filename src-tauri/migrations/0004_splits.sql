CREATE TABLE splits (
    id       TEXT PRIMARY KEY,
    name     TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    rules    TEXT NOT NULL DEFAULT '[]'
);

-- Default splits: Other (with rules) evaluated before Important (catch-all)
INSERT INTO splits (id, name, position, rules) VALUES
    ('important', 'Important', 0, '[]'),
    ('other', 'Other', 1, '[{"type":"is_newsletter"},{"type":"from_pattern","value":"noreply|no-reply|donotreply|do-not-reply|notifications|newsletter|mailer|alerts|updates|bounce|postmaster|auto-confirm"}]');

-- Store newsletter flag on messages for recategorization without re-fetching headers
ALTER TABLE messages ADD COLUMN is_newsletter INTEGER NOT NULL DEFAULT 0;
