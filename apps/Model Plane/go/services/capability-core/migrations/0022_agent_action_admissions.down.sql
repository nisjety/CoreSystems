-- Reverses 0022_agent_action_admissions.up.sql: drops the admission table and its seeded rows.
-- Nothing reads the table unless MODEL_ACTION_ADMISSION=table, so dropping it restores the
-- tickets.create-only behaviour exactly.
DROP TABLE IF EXISTS agent_action_admissions;
