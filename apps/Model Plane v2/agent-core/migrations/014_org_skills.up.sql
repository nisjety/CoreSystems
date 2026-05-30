-- Migration 014: org_skills
-- Stores LLM-synthesised skills that are auto-generated from successful trajectory clusters.
-- Each row represents one learned skill for one org/task_pattern pair.

CREATE TABLE IF NOT EXISTS org_skills (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id          UUID NOT NULL,
    name            VARCHAR(128) NOT NULL,
    task_pattern    VARCHAR(128) NOT NULL,

    -- The synthesised skill content (SKILL.md format)
    skill_md        TEXT NOT NULL,

    -- Provenance — which trajectories produced this skill
    source_trace_ids UUID[] NOT NULL DEFAULT '{}',

    -- Performance tracking
    usage_count     INT NOT NULL DEFAULT 0,
    success_count   INT NOT NULL DEFAULT 0,
    success_rate    NUMERIC(5, 4) NOT NULL DEFAULT 1.0,

    -- Lifecycle
    version         INT NOT NULL DEFAULT 1,
    deprecated_at   TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Look up applicable skills for an org
CREATE UNIQUE INDEX IF NOT EXISTS idx_org_skills_org_pattern
    ON org_skills (org_id, task_pattern)
    WHERE deprecated_at IS NULL;

-- Find skills worth recompressing (high usage, room for improvement)
CREATE INDEX IF NOT EXISTS idx_org_skills_active
    ON org_skills (org_id, success_rate, usage_count)
    WHERE deprecated_at IS NULL;
