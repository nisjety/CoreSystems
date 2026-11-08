-- Reverses 0071_message_post_kind.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only,
-- inside one transaction.
--
-- Dropping the column makes every human post indistinguishable from an agent turn: a
-- post would then be read into model context, agent memory and Dreaming as the owner's
-- words. Delete or archive the post rows first (post_kind = 'post'), or do not run this.
--
-- ORDER, all three steps:
--   1. roll session-core and model-gateway back to builds that do not read post_kind;
--   2. confirm no row has post_kind = 'post' (SELECT count(*) FROM messages WHERE post_kind = 'post');
--   3. run this file. A build that does read post_kind must be stopped before step 3, or
--      its reads fail with "column post_kind does not exist" until it restarts.
--
-- Safe to run twice (both statements are IF EXISTS).

BEGIN;
ALTER TABLE messages DROP CONSTRAINT IF EXISTS messages_post_kind_check;
ALTER TABLE messages DROP COLUMN IF EXISTS post_kind;
COMMIT;
