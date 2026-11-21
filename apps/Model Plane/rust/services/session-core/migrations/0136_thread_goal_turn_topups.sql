-- 0136: the goal turn top-up ledger (F-f09-11).
--
-- A goal turn whose run pauses (an approval, a question, a park) is settled with the usage of its first segment only;
-- each /continue segment after it runs under the same run and spends more. With MODEL_GATEWAY_GOAL_SEGMENT_TOPUP on,
-- model-gateway sends the segment's metered usage to session-core's TopUpGoalTurn, which adds it to the goal's counters
-- (tokens_used, cost_used_usd_micros; never turns_used). This table is what makes that debit idempotent: one row per
-- (goal, turn, segment_key), written in the same transaction as the counter update, so a retried segment adds nothing
-- the second time. segment_key is an opaque id of the continuation dispatch (the request id), never content.
--
-- Zero Data Retention: ids only, no content and no counts. A goal never exists on a zero-retention or waived thread
-- (0091), so neither does a top-up row. Erasure: rows go with their goal (ON DELETE CASCADE), which goes with its
-- thread and its org (delete_thread_rows and the org purge delete thread_goals; nothing else is needed).
--
-- Also adds a lookup index on thread_goal_turns (run_id): TopUpGoalTurn finds the turn by its run id and org, and the
-- existing thread_goal_turns_run_idx (0092) leads with goal_id, so it cannot serve that read. It is a new name on
-- purpose: reusing thread_goal_turns_run_idx under IF NOT EXISTS would silently skip the build. It is a plain
-- CREATE INDEX, not CONCURRENTLY (sqlx runs a migration inside a transaction, which refuses CONCURRENTLY), as 0059 and
-- 0061 are: the table holds one row per goal turn, so the build is brief, but writers to thread_goal_turns wait for it.
--
-- Additive only: a new empty table and an index. SWITCH: none for the migration; the RPC is off with
-- SESSION_CORE_THREAD_GOALS_ENABLED and the gateway's caller with MODEL_GATEWAY_GOAL_SEGMENT_TOPUP.
-- The reversal is in revert/0136_thread_goal_turn_topups.down.sql.

CREATE TABLE IF NOT EXISTS thread_goal_turn_topups (
    goal_id TEXT NOT NULL REFERENCES thread_goals(id) ON DELETE CASCADE,
    turn_no INTEGER NOT NULL CHECK (turn_no >= 1),
    -- Opaque id of the continuation dispatch that spent the usage; never content.
    segment_key TEXT NOT NULL CHECK (char_length(segment_key) BETWEEN 1 AND 128),
    org_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (goal_id, turn_no, segment_key)
);

CREATE INDEX IF NOT EXISTS thread_goal_turns_run_lookup_idx
    ON thread_goal_turns (run_id)
    WHERE run_id IS NOT NULL;
