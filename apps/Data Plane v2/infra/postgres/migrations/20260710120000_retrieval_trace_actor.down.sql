DROP INDEX IF EXISTS idx_retrieval_runs_actor;
ALTER TABLE retrieval_runs
    DROP COLUMN IF EXISTS actor_user_id;
