-- `archived` is shared by archive and trash actions for inbox visibility.
-- Keep a separate timestamp only after a remote Trash move succeeds, so
-- future retention cleanup can identify local data that is safe to purge.
ALTER TABLE threads ADD COLUMN trashed_at TEXT;

CREATE INDEX IF NOT EXISTS idx_threads_trashed_at
    ON threads(trashed_at)
    WHERE trashed_at IS NOT NULL;
