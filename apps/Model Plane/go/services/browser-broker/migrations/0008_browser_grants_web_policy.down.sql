-- Reverts 0008_browser_grants_web_policy.up.sql. Idempotent: DROP COLUMN IF
-- EXISTS, and a no-op when the table does not exist. Revert the broker build
-- with it: the build that reads or inserts web_policy fails once the column is
-- gone.
ALTER TABLE IF EXISTS browser_grants
    DROP COLUMN IF EXISTS web_policy;
