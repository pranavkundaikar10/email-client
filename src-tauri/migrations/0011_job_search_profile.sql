-- Keep the job-search classification separate from the general triage
-- category. This lets non-job email retain a useful AI summary without a
-- misleading hiring-stage badge.
ALTER TABLE email_analysis ADD COLUMN is_job_related INTEGER NOT NULL DEFAULT 0;
ALTER TABLE email_analysis ADD COLUMN job_category TEXT;
