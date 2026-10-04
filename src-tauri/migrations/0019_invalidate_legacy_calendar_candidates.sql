-- Earlier calendar candidates trusted model-generated RFC3339 timestamps.
-- They can contain invented years/timezones, so require a fresh analysis using
-- the deterministic resolver before offering an export action again.
UPDATE email_analysis SET calendar_event = NULL WHERE calendar_event IS NOT NULL;
