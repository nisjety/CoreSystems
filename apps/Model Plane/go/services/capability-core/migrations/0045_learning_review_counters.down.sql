-- Reverses 0045_learning_review_counters.up.sql.
--
-- Dropping the table loses the day's review counts. Turn LEARNING_REVIEW_COST_CONTROLS off on capability-core first:
-- with the switch on and the table gone, every review and every /learn is refused as budget_unavailable (fail closed).

DROP INDEX IF EXISTS learning_review_counters_org_idx;
DROP INDEX IF EXISTS learning_review_counters_expires_idx;
DROP TABLE IF EXISTS learning_review_counters;
