-- no-transaction
-- Reverts 0077_space_message_search_index.sql.
--
-- Dropping the index removes no message. SpaceHistoryService still answers (the query falls back
-- to a scan), only slower, so turn SPACE_MESSAGE_SEARCH_CORE off first on a large table.
-- CONCURRENTLY, so this file also runs outside a transaction.

DROP INDEX CONCURRENTLY IF EXISTS idx_messages_space_fts;
