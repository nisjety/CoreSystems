-- Reverses 0025_tool_policies_allow_needs_digest.up.sql.
--
-- Removes the constraint only. The revocations the up file made stay: a revoked row is
-- never read as a policy, and the API never revives one.

ALTER TABLE tool_policies
    DROP CONSTRAINT IF EXISTS tool_policies_allow_digest_present_check;
