-- Attribute newly persisted retrieval traces to the verified caller.
-- Existing rows remain NULL and therefore fail closed for ordinary users;
-- only an explicitly scoped org:data:read_all principal can inspect them.
ALTER TABLE retrieval_runs
    ADD COLUMN IF NOT EXISTS actor_user_id TEXT;

CREATE INDEX IF NOT EXISTS idx_retrieval_runs_actor
    ON retrieval_runs (org_id, actor_user_id, created_at DESC);
