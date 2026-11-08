-- 0091: thread goals (Wave 3 EP-7 S1; number assigned by integration round B).
--
-- A thread goal is a standing objective the owner sets on one of their threads,
-- with optional success criteria, recited to the model as data on every
-- agentic turn of the thread. One open goal per thread (D11): the partial
-- unique index below holds it across replicas. The budget columns exist from
-- S1 so the budget ledger (EP-7 S2) adds no column; nothing enforces them yet.
-- The states "met", "budget_reached", "turns_reached" and "blocked" are written
-- only by the S2/S3 code paths, never by the UpdateThreadGoal RPC.
--
-- Zero Data Retention: the objective and criteria are user content. A
-- zero-retention or waived credential is refused at the RPC (and at the
-- gateway, feature `thread_goal`), and a thread holding a waived run gets no
-- row, so no ZDR content is ever here. Space threads are refused (D8).
-- Erasure: rows go with their thread (ON DELETE CASCADE, and an explicit
-- statement in delete_thread_rows) and with their org (gdpr.rs purge).
--
-- Additive only. The table is new and empty, so no index needs CONCURRENTLY.

CREATE TABLE IF NOT EXISTS thread_goals (
    id TEXT PRIMARY KEY CHECK (id LIKE 'goal\_%'),
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    org_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    objective TEXT NOT NULL CHECK (char_length(objective) BETWEEN 1 AND 2000),
    -- A JSON array of at most 8 strings of at most 500 characters (checked by
    -- the store; the shape is held here).
    success_criteria JSONB NOT NULL DEFAULT '[]'::jsonb
        CHECK (jsonb_typeof(success_criteria) = 'array'
               AND jsonb_array_length(success_criteria) <= 8),
    token_budget BIGINT NULL
        CHECK (token_budget IS NULL OR token_budget BETWEEN 1 AND 5000000),
    cost_budget_usd_micros BIGINT NULL
        CHECK (cost_budget_usd_micros IS NULL OR cost_budget_usd_micros > 0),
    max_turns INTEGER NOT NULL DEFAULT 10 CHECK (max_turns BETWEEN 1 AND 50),
    status TEXT NOT NULL
        CHECK (status IN ('active', 'paused', 'met', 'budget_reached',
                          'turns_reached', 'blocked', 'cleared')),
    -- A closed token, never free text.
    status_reason TEXT NOT NULL DEFAULT ''
        CHECK (status_reason ~ '^[a-z0-9_]{0,64}$'),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    closed_at TIMESTAMPTZ NULL,
    -- An open goal has no close time; a closed one always has one.
    CHECK ((status IN ('active', 'paused')) = (closed_at IS NULL))
);

-- One open goal per thread (D11).
CREATE UNIQUE INDEX IF NOT EXISTS thread_goals_one_open_idx
    ON thread_goals (thread_id)
    WHERE status IN ('active', 'paused');

-- The latest goal of a thread (the read and the attention listing).
CREATE INDEX IF NOT EXISTS thread_goals_thread_idx
    ON thread_goals (thread_id, updated_at DESC);

-- A person's goals by state (the later goal views), and the org purge.
CREATE INDEX IF NOT EXISTS thread_goals_owner_idx
    ON thread_goals (org_id, user_id, status, updated_at);
