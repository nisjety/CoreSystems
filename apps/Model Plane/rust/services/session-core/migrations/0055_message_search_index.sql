-- no-transaction
-- Full-text index for conversation search (build plan WS17 S3).
--
-- SearchMessages matches on the same expression the index stores, so the planner
-- can use it: a tsvector of the first 64 KiB of each message, in the `simple`
-- configuration (no stemming, no stop words; the query side adds prefix matching).
-- Only user and assistant messages are indexed. Tool and system rows are not what a
-- person searches for, and the partial predicate keeps them out of the index.
--
-- The index is built whether or not SESSION_CORE_MESSAGE_SEARCH_ENABLED is on: the
-- switch gates the query path only, so enabling search later needs no migration.
-- CONCURRENTLY keeps writers unblocked while it builds, which is why this file runs
-- outside a transaction. A build that fails part way leaves an INVALID index behind;
-- drop it (revert/0055_message_search_index.down.sql) before running the migration again.
-- The reversal is in revert/0055_message_search_index.down.sql.

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_messages_content_fts
    ON messages USING GIN (to_tsvector('simple', left(content, 65536)))
    WHERE role IN ('user', 'assistant');
