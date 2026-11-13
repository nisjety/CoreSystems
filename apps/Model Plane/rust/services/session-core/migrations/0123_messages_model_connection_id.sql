-- 0123: the customer-gateway connection a turn was written under (W4-BYO, GAP-W4-BYO-1).
--
-- A turn served by one of the caller's own customer-gateway connections (MODEL_GATEWAY_BYO_GATEWAY) names an
-- Integration Core connection id. Without a stored id, a regenerate, edit or retry of that turn, reloaded from
-- history, carried no connection and went to the platform chain under the stored model id, where a platform
-- provider could serve it. With the id on the turn, the gateway replays on the same connection and refuses
-- anything else, typed.
--
-- The column holds the opaque connection id only: never a key, a token or a URL. NULL for every row written
-- before this migration, for a platform turn and for any turn written while the switch is off. No backfill.
--
-- Zero Data Retention: a zero-retention caller cannot append a message at all, so no ZDR turn ever holds an id,
-- and a waived run keeps no turn row. The id is a routing fact about a turn that is already stored; it adds no
-- new content store. Erasure: it lives on the message row, so deleting the message or the org's threads (gdpr.rs)
-- deletes it with the row.
--
-- Additive only: one nullable column. The reversal is in revert/0123_messages_model_connection_id.down.sql.

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS model_connection_id TEXT NULL
        CHECK (model_connection_id IS NULL OR length(model_connection_id) BETWEEN 1 AND 128);
