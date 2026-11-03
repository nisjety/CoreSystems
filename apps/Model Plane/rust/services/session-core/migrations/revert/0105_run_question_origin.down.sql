-- Reverses 0105_run_question_origin.sql: drops run_questions.origin_label and
-- run_questions.origin (their column CHECKs go with them) and the label-shape CHECK.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top
-- level of migrations/. Run it by hand, against the database only, inside one transaction.
--
-- Dropping the columns loses which questions an MCP server asked: after the revert every
-- question reads as the model's. Before running it, roll session-core back to a build that does
-- not read the columns (its RUN_QUESTION_COLUMNS would otherwise fail every question query), and
-- confirm no server question is still waiting, read-only:
--
--   SELECT count(*) FROM run_questions
--    WHERE origin <> 'model' AND state = 'requested';
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

ALTER TABLE run_questions
    DROP CONSTRAINT IF EXISTS run_questions_origin_label_chk;

ALTER TABLE run_questions
    DROP COLUMN IF EXISTS origin_label;

ALTER TABLE run_questions
    DROP COLUMN IF EXISTS origin;

COMMIT;
