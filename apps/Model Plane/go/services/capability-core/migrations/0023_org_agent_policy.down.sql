-- Reverses 0023_org_agent_policy.up.sql.
--
-- Dropping the tables loses every stored policy and its history; the audit rows in
-- registry_audit_log remain. Nothing reads these tables before WS25-S9 is switched on,
-- so drop them only with ORG_AGENT_POLICY_API off.

DROP TABLE IF EXISTS org_agent_policy_history;
DROP TABLE IF EXISTS org_agent_policy;
