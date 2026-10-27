-- Reverses 0073_thread_space_kind.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only,
-- inside one transaction.
--
-- Dropping the column loses which Space threads are personal. Without it a build that
-- still reads space_kind fails with "column space_kind does not exist", and a build that
-- does not read it treats every Space thread as a plain thread (memory containment off).
-- Roll session-core back first, then run this file.
--
-- Safe to run twice.

BEGIN;
ALTER TABLE threads DROP COLUMN IF EXISTS space_kind;
COMMIT;
