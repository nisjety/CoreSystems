-- Reverses 0083_memory_declassifications.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- Drops the ledger. Roll session-core back first (a build that writes the ledger fails without the table).
-- Room rows labelled 'declassified' in agent_memory are not touched: they stay room memory; only the ledger
-- that records their origin and retraction is lost.
--
-- Safe to run twice.

BEGIN;
DROP INDEX IF EXISTS memory_declassifications_room_idx;
DROP INDEX IF EXISTS memory_declassifications_idem_uq;
DROP TABLE IF EXISTS memory_declassifications;
COMMIT;
