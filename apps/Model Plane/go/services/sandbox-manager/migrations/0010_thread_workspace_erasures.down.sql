-- Reverts 0010_thread_workspace_erasures.up.sql. Dropping the table forgets every
-- pending blob erasure, so run it only once the blobs of those threads are
-- removed (or the operator accepts the orphans).
DROP INDEX IF EXISTS thread_workspace_erasures_requested_idx;
DROP TABLE IF EXISTS thread_workspace_erasures;
