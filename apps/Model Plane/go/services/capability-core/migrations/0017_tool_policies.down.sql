-- Reverses 0017_tool_policies.up.sql.
--
-- Dropping the table loses every stored policy and its history; the audit rows in
-- registry_audit_log remain. Turn TOOL_POLICY_ENFORCED off first: with it off the
-- policies are already ignored, so nothing changes at the moment of the drop.

DROP TABLE IF EXISTS tool_policies;

ALTER TABLE capabilities
    DROP COLUMN IF EXISTS policy_relaxable;
