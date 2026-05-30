-- Migration 013: agent_trajectories
-- Records per-run execution traces for self-improvement, Letta sync, and RL export.

CREATE TABLE IF NOT EXISTS agent_trajectories (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id          UUID NOT NULL,
    org_id          UUID NOT NULL,
    session_id      TEXT,

    -- What the agent was asked to do and how we normalise it
    task_goal       TEXT NOT NULL,
    task_pattern    VARCHAR(128) NOT NULL,  -- normalised canonical form, e.g. 'summarise_document'

    -- LLM usage
    model           VARCHAR(128),
    tokens_in       INT NOT NULL DEFAULT 0,
    tokens_out      INT NOT NULL DEFAULT 0,
    cost_usd        NUMERIC(12, 8) NOT NULL DEFAULT 0,

    -- Structured trace
    planned_actions JSONB NOT NULL DEFAULT '[]',
    executed_actions JSONB NOT NULL DEFAULT '[]',

    -- Outcome
    outcome         VARCHAR(16) NOT NULL DEFAULT 'success'
                    CHECK (outcome IN ('success', 'partial', 'failure')),
    duration_sec    NUMERIC(10, 3),
    skills_used     TEXT[] NOT NULL DEFAULT '{}',

    -- Letta sync state
    letta_stored    BOOLEAN NOT NULL DEFAULT FALSE,

    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Lookup by org + pattern for skill compressor and org insights
CREATE INDEX IF NOT EXISTS idx_trajectories_org_pattern
    ON agent_trajectories (org_id, task_pattern);

-- Time-series queries for org insights
CREATE INDEX IF NOT EXISTS idx_trajectories_org_outcome_time
    ON agent_trajectories (org_id, outcome, created_at DESC);

-- Full-text / JSON search on planned actions
CREATE INDEX IF NOT EXISTS idx_trajectories_planned_gin
    ON agent_trajectories USING GIN (planned_actions);

-- Letta sync sweep
CREATE INDEX IF NOT EXISTS idx_trajectories_letta_sync
    ON agent_trajectories (letta_stored, created_at)
    WHERE letta_stored = FALSE;
