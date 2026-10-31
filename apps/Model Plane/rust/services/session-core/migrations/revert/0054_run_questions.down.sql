-- Reverts 0054_run_questions.sql. Dropping the table discards every question and
-- answer, so a rollback loses what owners were asked and said. Nothing else holds a
-- reference to it: no approval, plan or run row points at a question.

DROP INDEX IF EXISTS run_questions_idempotency_idx;
DROP INDEX IF EXISTS run_questions_pending_idx;
DROP INDEX IF EXISTS run_questions_run_idx;
DROP TABLE IF EXISTS run_questions;
