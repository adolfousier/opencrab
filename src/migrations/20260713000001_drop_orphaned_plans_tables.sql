-- Drop the orphaned plans / plan_tasks tables.
--
-- Plan Mode originally persisted plans and their tasks to SQLite (added in the
-- v0.1.0 initial release, 2026-02-14). The plan-lifecycle redesign moved all
-- plan state to per-session files: .opencrabs_plan_<session>.json holds the
-- structured state (status, tasks, progress) and the sibling .md holds the
-- design prose. PlanService and PlanRepository were retired with it, so nothing
-- has read or written these tables since. They are dead schema that only
-- confuses anyone inspecting the database.
--
-- Drop the child table first: plan_tasks has a foreign key into plans.

DROP TABLE IF EXISTS plan_tasks;
DROP TABLE IF EXISTS plans;
