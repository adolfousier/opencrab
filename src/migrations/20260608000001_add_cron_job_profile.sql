-- Record the profile a cron job was created in so the scheduler never runs a
-- job under the wrong profile's brain/config/tools (#182).
--
-- NULL = legacy job created before profile stamping. The scheduler treats NULL
-- as "run anywhere" for backward compatibility (the per-profile DB already
-- isolates it). Newly created jobs are always stamped: the base profile is
-- stored as the literal 'default'.
ALTER TABLE cron_jobs ADD COLUMN profile_name TEXT;
