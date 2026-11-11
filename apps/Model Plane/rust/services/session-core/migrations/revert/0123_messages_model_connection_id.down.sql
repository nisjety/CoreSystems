-- Reverses 0123_messages_model_connection_id.sql: drops the per-turn customer-gateway connection id.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: set MODEL_GATEWAY_BYO_GATEWAY to anything but true in model-gateway (or roll session-core back to a
-- build that does not select the column) first, or the conversation read fails on the missing column. The
-- column holds opaque connection ids only, so nothing needs copying first; after the drop a gateway turn
-- reloaded from history has no stored connection again.
--
-- Safe to run twice (the DROP is IF EXISTS).

BEGIN;

ALTER TABLE messages DROP COLUMN IF EXISTS model_connection_id;

COMMIT;
