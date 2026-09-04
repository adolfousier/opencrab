-- Ship-plane marker for bot replies (#91, cross-turn glue).
-- The cross-turn glue rung re-edits the conversation's LAST bot message to
-- carry the suggestion controls. That is only safe on the rich MARKDOWN
-- plane, where the stored content IS the shipped body and a re-edit through
-- edit_rich_markdown re-renders it identically (tables intact, #79). Classic
-- bodies would be re-rendered as rich (visual churn), and legacy rich-html
-- bodies have no stored source to re-send — so the glue lookup filters on
-- this marker and everything else falls back to the standalone bubble.
-- Nullable: NULL = classic or pre-marker row (never a glue target).

ALTER TABLE channel_messages ADD COLUMN ship_plane TEXT;
