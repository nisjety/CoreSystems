-- Rollback migration 001
DROP TRIGGER IF EXISTS trg_plans_updated ON plans;
DROP TRIGGER IF EXISTS trg_todos_updated ON todos;
DROP TRIGGER IF EXISTS trg_runs_updated  ON agent_runs;
DROP FUNCTION IF EXISTS set_updated_at();

DROP TABLE IF EXISTS approvals CASCADE;
DROP TABLE IF EXISTS plans     CASCADE;
DROP TABLE IF EXISTS todos     CASCADE;
DROP TABLE IF EXISTS agent_runs CASCADE;
DROP TABLE IF EXISTS schema_migrations;
