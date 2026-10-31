-- 0004_rejected_terminal.sql
-- A notice whose download was refused for its content (too large, or an empty body)
-- is retried after a cool-off, and after its third refusal it is terminal: it counts
-- toward its day's completeness in a bucket of its own, as a 404 does, so that the day
-- can become done. This adds the bucket to sync_days and puts it in the two sums the
-- day constraints check: a done day adds up only with it, and a capped day counts none.
-- Forward-only: 0002 is not edited. The two constraints are replaced under their old names.
-- Runs inside the caller's transaction (migrate.go, applyOne), so no BEGIN, COMMIT or ROLLBACK.
-- migrate_concurrency_test.go drops sync_days with the other raw store tables before each
-- round, which removes this column and these constraints with it.

ALTER TABLE sync_days
    ADD COLUMN rejected INTEGER NOT NULL DEFAULT 0 CHECK (rejected >= 0);

ALTER TABLE sync_days DROP CONSTRAINT sync_days_done_adds_up;
ALTER TABLE sync_days ADD CONSTRAINT sync_days_done_adds_up CHECK (
    state <> 'done'
    OR (num_total IS NOT NULL AND num_accessible IS NOT NULL
        AND num_total = num_accessible AND error_code IS NULL AND unresolved = 0
        AND held + stored + not_found + rejected = num_accessible));

ALTER TABLE sync_days DROP CONSTRAINT sync_days_capped_is_saturated;
ALTER TABLE sync_days ADD CONSTRAINT sync_days_capped_is_saturated CHECK (
    state <> 'capped'
    OR (num_total IS NOT NULL AND num_accessible IS NOT NULL
        AND num_total > num_accessible AND error_code = 'saturated'
        AND held = 0 AND stored = 0 AND not_found = 0 AND rejected = 0 AND unresolved = 0));

COMMENT ON COLUMN sync_days.rejected IS 'Notices of the day that were refused for their content (too large, or an empty body) three times: terminal, never downloaded again. Counts describe the latest attempt.';
