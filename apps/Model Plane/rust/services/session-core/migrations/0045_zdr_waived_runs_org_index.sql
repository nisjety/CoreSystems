-- Org-scoped counts of waived runs (Z-21a, zero data retention).
--
-- The org admin view counts a user's waived runs per connection and reads the
-- start time of the latest one (CountWaivedRuns). Only waived runs are indexed,
-- so that read grows with the waived runs, not with `runs`. The predicate is the
-- literal 'zdr_waived', and the query spells the class the same way, so the
-- planner can match the partial index.
--
-- A plain CREATE INDEX, as 0044 creates its index: it blocks writes to `runs`
-- while it builds, so run it in a quiet window on a large database.
--
-- The reversal is in revert/0045_zdr_waived_runs_org_index.down.sql.

CREATE INDEX IF NOT EXISTS runs_zdr_waived_org_connection_idx
    ON runs (org_id, waiver_connection_id, user_id)
    INCLUDE (created_at)
    WHERE retention_class = 'zdr_waived';
