-- Revert 0087: forgets which runs read external content. Dreaming then treats those runs'
-- assistant messages as eligible again.
DROP INDEX IF EXISTS idx_runs_thread_external;
ALTER TABLE runs DROP COLUMN IF EXISTS external_content_at;
