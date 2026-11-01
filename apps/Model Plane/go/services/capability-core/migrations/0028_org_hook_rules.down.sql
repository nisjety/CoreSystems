-- Reverses 0028_org_hook_rules.up.sql.
--
-- Dropping the table loses every durable rule and its revoked history. The audit rows in
-- registry_audit_log and the revision chain in org_agent_policy_history remain. A gateway with
-- HOOKS_DURABLE on could no longer read its rules, so drop the table only with HOOKS_DURABLE off on
-- every gateway.

DROP INDEX IF EXISTS org_hook_rules_org_active_idx;
DROP INDEX IF EXISTS org_hook_rules_active_key;
DROP TABLE IF EXISTS org_hook_rules;
