-- 0128: the subscription connection a turn was written under (DEC-V-R8; verification finding R8-rel1).
--
-- A turn served by the caller's own ChatGPT subscription (openai-codex-subscription) names an Integration Core
-- subscription connection id. No turn kept it, so a side question on a subscription thread carried no connection
-- and was answered on the platform chain. With the id on the turn, the side chat answers on the same subscription
-- connection and refuses anything else, typed (model-gateway side_chat_routes.rs).
--
-- The column holds the opaque connection id only: never a key, a token or a URL. NULL for every row written before
-- this migration (no backfill: the id was never kept, so an older subscription turn is not identified), for a
-- platform turn and for a customer-gateway turn. A send without an id never touches the column.
--
-- Zero Data Retention: a zero-retention caller cannot append a message at all, so no ZDR turn ever holds an id, and
-- a waived run keeps no turn row. The id is a routing fact about a turn that is already stored; it adds no new
-- content store. Erasure: it lives on the message row, so deleting the message or the org's threads (gdpr.rs)
-- deletes it with the row.
--
-- Additive only: one nullable column. The reversal is in revert/0128_messages_subscription_connection_id.down.sql.

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS subscription_connection_id TEXT NULL
        CHECK (subscription_connection_id IS NULL OR length(subscription_connection_id) BETWEEN 1 AND 128);
