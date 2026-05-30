DROP INDEX IF EXISTS idx_documents_outbox_unpublished;
DROP TABLE IF EXISTS documents_outbox;
DROP TABLE IF EXISTS org_versions;
DROP INDEX IF EXISTS uq_agent_retrieval_configs_org_agent;
DROP TABLE IF EXISTS agent_retrieval_configs;
ALTER TABLE retrieval_runs DROP COLUMN IF EXISTS mode_mix_applied;
