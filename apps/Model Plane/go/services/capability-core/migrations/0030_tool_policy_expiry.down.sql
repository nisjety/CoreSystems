-- Reverses 0030_tool_policy_expiry.up.sql.
--
-- Dropping the column makes every lapsed rule permanent again: an allow that had lapsed reads as live
-- once the column is gone. Turn TOOL_POLICY_ENFORCED off first, then look at what would come back,
-- read-only:
--
--   SELECT count(*) FROM tool_policies
--   WHERE decision = 'allow' AND revoked_at IS NULL AND expires_at <= now();
--
-- Revoke those rows before running this file if they must not come back.

ALTER TABLE tool_policies DROP CONSTRAINT IF EXISTS tool_policies_expiry_allow_check;
ALTER TABLE tool_policies DROP COLUMN IF EXISTS expires_at;
