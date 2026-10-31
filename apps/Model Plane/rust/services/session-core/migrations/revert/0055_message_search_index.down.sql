-- no-transaction
-- Reverts 0055_message_search_index.sql.
--
-- Dropping the index removes no message. With the index gone, SearchMessages still
-- answers, only slower, so the reversal is safe to run while the switch is on.
-- CONCURRENTLY, so this file also runs outside a transaction.

DROP INDEX CONCURRENTLY IF EXISTS idx_messages_content_fts;
