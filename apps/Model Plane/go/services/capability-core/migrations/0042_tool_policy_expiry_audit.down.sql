-- Reverses 0042_tool_policy_expiry_audit.up.sql.
--
-- Drops the sweeper's index and the column that records an expiry as audited. The tool_policy_rule_expired
-- rows already written stay in registry_audit_log (it is append-only). With the column gone a later
-- sweeper would write them again, so turn TOOL_POLICY_RULE_AUDIT_ENABLED off before running this file.

DROP INDEX IF EXISTS tool_policies_expiry_due_idx;
ALTER TABLE tool_policies DROP COLUMN IF EXISTS expiry_audited_at;
