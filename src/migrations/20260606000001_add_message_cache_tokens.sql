-- Add cache token columns to messages table for prompt caching tracking
-- These columns were in the initial schema but dropped by the modernize migration

ALTER TABLE messages ADD COLUMN cache_creation_tokens INTEGER;
ALTER TABLE messages ADD COLUMN cache_read_tokens INTEGER;
