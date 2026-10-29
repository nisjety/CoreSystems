-- Migration 004 rollback: drop orchestration tables in reverse dependency order

DROP TABLE IF EXISTS subagent_lineage_edges;
DROP TABLE IF EXISTS todos;
DROP TABLE IF EXISTS approvals;
DROP TABLE IF EXISTS plan_steps;
DROP TABLE IF EXISTS plans;
