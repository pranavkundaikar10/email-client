-- Distinguish a fresh operation from one whose IMAP response may have been
-- interrupted by sleep or network loss. Retries of the latter reconcile the
-- remote destination before replaying a Trash move.
ALTER TABLE mail_operations ADD COLUMN remote_attempted_at TEXT;
