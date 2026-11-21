-- Reverts 0009_browser_grants_read_only.up.sql. Idempotent: DROP COLUMN IF
-- EXISTS, and a no-op when the table does not exist. Revert the broker build
-- with it: the build that reads or inserts read_only fails once the column is
-- gone.
--
-- Dropping the column turns every stored read-only grant into a full-access one.
-- Run it only with BROWSER_BROKER_DURABLE_READ_ONLY_ENABLED off and the live
-- read-only grants revoked or expired first.
ALTER TABLE IF EXISTS browser_grants
    DROP COLUMN IF EXISTS read_only;
