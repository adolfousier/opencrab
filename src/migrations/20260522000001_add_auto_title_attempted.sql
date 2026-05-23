-- Add auto_title_attempted flag to sessions table.
-- Prevents duplicate auto-title generation for the same session.
-- Existing rows get 0 (false) so old untitled sessions remain eligible.
ALTER TABLE sessions ADD COLUMN auto_title_attempted INTEGER NOT NULL DEFAULT 0;
