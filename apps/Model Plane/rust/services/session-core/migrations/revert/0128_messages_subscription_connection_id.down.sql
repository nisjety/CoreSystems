-- Reverses 0128_messages_subscription_connection_id.sql: drops the per-turn subscription connection id.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: roll session-core back to a build without this column first (its conversation read and its append select
-- and write it), or the conversation read fails on the missing column. The column holds opaque connection ids only,
-- so nothing needs copying first; after the drop a subscription turn reloaded from history has no stored connection
-- again.
--
-- Safe to run twice (the DROP is IF EXISTS).

BEGIN;

ALTER TABLE messages DROP COLUMN IF EXISTS subscription_connection_id;

COMMIT;
