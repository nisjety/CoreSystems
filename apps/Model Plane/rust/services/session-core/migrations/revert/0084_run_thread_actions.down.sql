-- Reverses 0084_run_thread_actions.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top
-- level of migrations/. Run it by hand, against the database only, inside one transaction.
--
-- Roll session-core back first: a build that writes receipts or reads the source columns fails on a
-- missing table or column. Drops the receipts (digests only), the two source columns and the card
-- column. Renamed titles and pins stay: they are ordinary thread data. Safe to run twice.

BEGIN;
DROP TABLE IF EXISTS run_thread_action_receipts;
DROP INDEX IF EXISTS messages_card_run_idx;
ALTER TABLE messages DROP COLUMN IF EXISTS card_run_id;
ALTER TABLE threads DROP COLUMN IF EXISTS pinned_source;
ALTER TABLE threads DROP COLUMN IF EXISTS title_source;
COMMIT;
