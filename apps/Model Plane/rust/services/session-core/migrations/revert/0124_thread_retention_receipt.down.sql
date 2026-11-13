-- Reverses 0124_thread_retention_receipt.sql: drops the thread retention receipt.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: set SESSION_THREAD_RECEIPT_ENFORCE to anything but true in session-core (or roll session-core back to
-- a build that does not read or write the columns) first, or CreateThread and AppendMessage fail on the
-- missing columns. The columns hold a class word and opaque ids only, so nothing needs copying first; after the
-- drop every thread reads as persistent again, which is what a NULL receipt meant.
--
-- Safe to run twice (every statement is IF EXISTS).

BEGIN;

ALTER TABLE threads DROP CONSTRAINT IF EXISTS threads_retention_class_check;
ALTER TABLE threads
    DROP COLUMN IF EXISTS waiver_connection_id,
    DROP COLUMN IF EXISTS waiver_ack_id,
    DROP COLUMN IF EXISTS retention_class;

COMMIT;
