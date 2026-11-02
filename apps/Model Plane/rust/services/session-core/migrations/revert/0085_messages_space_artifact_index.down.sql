-- no-transaction
-- Reverts 0085_messages_space_artifact_index.sql.
--
-- Dropping the index removes no message. ListSpaceArtifacts still answers, only slower, so
-- the reversal is safe to run while the room artifact API is on. CONCURRENTLY, so this file
-- also runs outside a transaction.

DROP INDEX CONCURRENTLY IF EXISTS idx_messages_space_artifacts;
