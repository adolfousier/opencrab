-- Origin forum topic for pending requests (#1457).
--
-- The channel layer parses message_thread_id on every inbound Telegram
-- message, but the pending-request row had nowhere to put it — so a
-- /rebuild fired from a forum topic reported completion into the chat's
-- default topic (General) instead (#1457). Nullable: legacy rows and
-- non-topic channels (DMs, Discord, TUI) carry NULL and keep the exact
-- pre-existing delivery behavior.
ALTER TABLE pending_requests ADD COLUMN channel_thread_id TEXT;
