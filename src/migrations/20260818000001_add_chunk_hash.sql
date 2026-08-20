-- Add chunk_hash column to content_vectors for per-chunk caching (#1107).
--
-- Without this, every append to a brain file re-embeds the entire file
-- (~100 chunks) because the document hash changes. With per-chunk hashes,
-- only chunks whose content actually changed are re-embedded.

ALTER TABLE content_vectors ADD COLUMN chunk_hash TEXT;
