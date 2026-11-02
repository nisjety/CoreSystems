-- no-transaction
-- Full-text index for Space message search (build plan WS13-S1).
--
-- SpaceHistoryService.SearchMessages (src/space/history.rs) matches on the same expression the
-- index stores, character for character, so the planner can use it: a tsvector of the first
-- 100,000 characters of each message, in the `simple` configuration (no stemming, no stop words;
-- the query side adds prefix matching). The partial predicate keeps personal-thread messages
-- (space_id IS NULL, indexed by 0055) and tool or system rows out of this index. A member's room
-- post (post_kind = 'post') is a user row and is indexed.
--
-- The index is built whether or not SPACE_MESSAGE_SEARCH_CORE is on: the switch gates the query
-- path only, so enabling search later needs no migration. Setting the switch to 0 does not remove
-- the index or its write cost; the down file does.
-- CONCURRENTLY keeps writers unblocked while it builds, which is why this file runs outside a
-- transaction (same convention as 0055 and 0085). A build that fails part way leaves an INVALID
-- index behind; drop it (revert/0077_space_message_search_index.down.sql) before running the
-- migration again. Erasure deletes message rows directly, so the index needs no purge hook.
-- The reversal is in revert/0077_space_message_search_index.down.sql.

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_messages_space_fts
    ON messages USING GIN (to_tsvector('simple', left(content, 100000)))
    WHERE space_id IS NOT NULL AND role IN ('user', 'assistant');
