-- Reverses 0132_attention_sweep_marks.sql: drops the attention sweep's high-water mark table.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: roll session-core back to a build without the attention sweep's mark first. A build that still reads the
-- mark fails its pass on the missing table and writes nothing. Dropping the table loses only the mark: the next
-- pass reads the default ten-minute window again.
--
-- ORDER:
--   1. run this file;
--   2. delete the 0132 row from _sqlx_migrations so the next boot re-applies it.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;
DROP TABLE IF EXISTS attention_sweep_marks;
COMMIT;
