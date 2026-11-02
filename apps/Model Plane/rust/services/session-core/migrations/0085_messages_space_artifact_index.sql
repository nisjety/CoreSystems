-- no-transaction
-- Index for the Space room artifact aggregate (build plan WS07-S9).
--
-- ListSpaceArtifacts (src/space/read_service.rs) reads one Space's assistant messages that
-- carry metadata.artifacts, newest first. The partial predicate keeps every other message
-- out of the index, and the key order matches the keyset the query pages on, so the planner
-- walks one Space's artifact-bearing turns without touching the rest of the table.
--
-- The index is built whether or not SPACE_ROOM_ARTIFACTS_API is on: the flag gates the
-- gateway route only. CONCURRENTLY keeps writers unblocked while it builds, which is why this
-- file runs outside a transaction (same convention as 0055). A build that fails part way
-- leaves an INVALID index behind; drop it (revert/0085_messages_space_artifact_index.down.sql)
-- before running the migration again. The reversal is in that revert file.

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_messages_space_artifacts
    ON messages (space_id, created_at DESC, id DESC)
    WHERE space_id IS NOT NULL AND role = 'assistant' AND jsonb_exists(metadata, 'artifacts');
