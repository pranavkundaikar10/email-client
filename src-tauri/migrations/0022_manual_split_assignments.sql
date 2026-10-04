-- A deliberate, per-thread split assignment takes precedence over automatic
-- split rules. It is local-only and can be cleared when its split is removed.
ALTER TABLE threads ADD COLUMN manual_category TEXT;

CREATE INDEX IF NOT EXISTS idx_threads_manual_category
  ON threads(manual_category)
  WHERE manual_category IS NOT NULL;
