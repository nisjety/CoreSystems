-- 0092: the thread goal turn ledger (Wave 3 EP-7 S2; number assigned by integration round B).
--
-- One row per continuation turn a goal admits: the gateway claims the turn before the run is
-- prepared, binds the run's id once it exists, and settles it from the run's own usage when the
-- run ends. At most one turn of a goal is unsettled at a time (the claim is taken under the goal's
-- row lock); a claim whose lease ran out (a crashed replica) is settled `lease_expired` by the next
-- claim. The goal's counters (`tokens_used`, `cost_used_usd_micros`, `turns_used`) are cached on
-- `thread_goals` and written in the same transaction as the settle, so the bar reads one row.
--
-- Budget semantics (owner decision D2, recommendation taken): only metered tokens count, as
-- cost-core and the run ceiling count them; a subscription turn spends none, so every goal also
-- has its turn cap. The judge's tokens (EP-7 S3) count against the goal. At 100% no new
-- continuation is claimed; the turn in flight finishes under its own per-run ceiling.
--
-- Zero Data Retention: no content. Ids, counts and closed tokens only. A goal never exists on a
-- zero-retention or waived thread (0091), so neither does a turn. Erasure: rows go with their
-- goal (ON DELETE CASCADE), which goes with its thread and its org.
--
-- Additive only: a new empty table, and three columns with constant defaults on thread_goals
-- (a metadata-only change, no table rewrite). The verdict columns of EP-7 S3 are not folded in:
-- S3 was not built with this slice, so they keep 0093.

CREATE TABLE IF NOT EXISTS thread_goal_turns (
    goal_id TEXT NOT NULL REFERENCES thread_goals(id) ON DELETE CASCADE,
    -- 1 for a goal's first turn, one higher for every later claim.
    turn_no INTEGER NOT NULL CHECK (turn_no >= 1),
    -- The run the turn started; NULL until it is bound, and for a turn that never started one.
    run_id TEXT NULL,
    org_id TEXT NOT NULL,
    claimed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- A claim not settled by then is settled `lease_expired` by the next claim (D11: 30 minutes).
    claim_expires_at TIMESTAMPTZ NOT NULL,
    settled_at TIMESTAMPTZ NULL,
    outcome TEXT NULL
        CHECK (outcome IS NULL OR outcome IN ('completed', 'failed', 'awaiting_approval',
                                              'parked', 'awaiting_input', 'cancelled',
                                              'unknown', 'lease_expired')),
    metered_input_tokens BIGINT NOT NULL DEFAULT 0 CHECK (metered_input_tokens >= 0),
    metered_output_tokens BIGINT NOT NULL DEFAULT 0 CHECK (metered_output_tokens >= 0),
    -- The turn's spend in micro-USD; NULL when it could not be priced.
    cost_usd_micros BIGINT NULL CHECK (cost_usd_micros IS NULL OR cost_usd_micros >= 0),
    -- A closed token ("metered", "subscription", ...); empty when unknown.
    cost_basis TEXT NOT NULL DEFAULT '' CHECK (cost_basis ~ '^[a-z_]{0,32}$'),
    judge_tokens BIGINT NOT NULL DEFAULT 0 CHECK (judge_tokens >= 0),
    -- Where the usage came from: the run's own answer, the round tally of a run that ended
    -- without one (register D2-005), the cost ledger (later), or none.
    usage_source TEXT NOT NULL DEFAULT 'run_response'
        CHECK (usage_source IN ('run_response', 'progress', 'ledger', 'none')),
    -- Whether the turn counted toward `max_turns` (a turn that never started a run does not).
    counted BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (goal_id, turn_no),
    -- A settled turn always has an outcome; an open one never does.
    CHECK ((settled_at IS NULL) = (outcome IS NULL))
);

-- A run settles one turn of a goal: the settle is idempotent on it.
CREATE UNIQUE INDEX IF NOT EXISTS thread_goal_turns_run_idx
    ON thread_goal_turns (goal_id, run_id)
    WHERE run_id IS NOT NULL;

-- The claim's "is a turn of this goal still open" read.
CREATE INDEX IF NOT EXISTS thread_goal_turns_open_idx
    ON thread_goal_turns (goal_id)
    WHERE settled_at IS NULL;

-- The cached counters, derived from the settled turns in the settle's own transaction.
ALTER TABLE thread_goals
    ADD COLUMN IF NOT EXISTS tokens_used BIGINT NOT NULL DEFAULT 0 CHECK (tokens_used >= 0);
ALTER TABLE thread_goals
    ADD COLUMN IF NOT EXISTS cost_used_usd_micros BIGINT NOT NULL DEFAULT 0
        CHECK (cost_used_usd_micros >= 0);
ALTER TABLE thread_goals
    ADD COLUMN IF NOT EXISTS turns_used INTEGER NOT NULL DEFAULT 0 CHECK (turns_used >= 0);
