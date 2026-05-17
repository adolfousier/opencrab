-- Heal cron_jobs rows whose TEXT columns were stored as BLOB by an
-- earlier sqlx-era binding. SQLite's flexible typing accepted the
-- byte-buffer inserts despite the column being declared `TEXT NOT NULL`,
-- and `row.get::<String, _>("prompt")` then failed on those rows in
-- `CronJobRepository::list_all`. Symptom: Mission Control's schedule
-- panel rendered "No scheduled jobs." while `opencrabs cron list` exited
-- code 1 with no stderr (until the diagnostic improvements in this same
-- commit landed). CAST(x AS TEXT) is a no-op on existing TEXT values and
-- fixes the storage class on BLOBs in place.

UPDATE cron_jobs SET prompt    = CAST(prompt    AS TEXT) WHERE typeof(prompt)    = 'blob';
UPDATE cron_jobs SET name      = CAST(name      AS TEXT) WHERE typeof(name)      = 'blob';
UPDATE cron_jobs SET cron_expr = CAST(cron_expr AS TEXT) WHERE typeof(cron_expr) = 'blob';
UPDATE cron_jobs SET timezone  = CAST(timezone  AS TEXT) WHERE typeof(timezone)  = 'blob';
UPDATE cron_jobs SET thinking  = CAST(thinking  AS TEXT) WHERE typeof(thinking)  = 'blob';
