-- Add deliver_api_key column to cron_jobs for HTTP webhook auth.
-- Generic: any Bearer token the job needs for delivery, stored per-job.
ALTER TABLE cron_jobs ADD COLUMN deliver_api_key TEXT;
