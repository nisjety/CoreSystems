-- orchestrator-core durable feedback ratings.
--
-- Replaces the in-memory `sync.Mutex` + `map` accumulator that was wiped on
-- every restart (AGENT_QUALITY_PLAN_2026-07-29 §1.2). One row per
-- (org_id, run_id, user_id, skill_id):
--
--   * skill_id = ''      → the run-level rating (answer quality for that turn).
--   * skill_id = 'x'     → the rating attributed to skill x, which was injected
--                          into that turn. This is what lets a thumbs-down
--                          demote the skill that steered a bad answer.
--
-- The primary key makes a re-rating an UPSERT rather than a second sample: a
-- user who clicks thumbs-up and then thumbs-down leaves one row with the latest
-- value, not two contradictory ones.
--
-- Aggregation (good-ratio per skill) is a GROUP BY at read time so the detail is
-- never lost and the promotion bar can be re-derived with new parameters.
CREATE TABLE IF NOT EXISTS feedback_ratings (
    org_id     text        NOT NULL,
    run_id     text        NOT NULL,
    user_id    text        NOT NULL DEFAULT '',
    -- '' = the run-level rating; otherwise an injected skill id.
    skill_id   text        NOT NULL DEFAULT '',
    from_scope text        NOT NULL DEFAULT '',
    to_scope   text        NOT NULL DEFAULT '',
    -- Canonical vocabulary only. model-gateway normalises client vocabularies
    -- (a chat thumbs-up/down included) before publishing, so a non-canonical
    -- value can never enter the sample total and silently deflate a score.
    rating     text        NOT NULL
                           CHECK (rating IN ('good', 'acceptable', 'poor')),
    note       text        NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, run_id, user_id, skill_id)
);

-- Promotion sweep: aggregate skill-attached ratings per tenant. Ratings are
-- never pooled across orgs, so org_id leads the index.
CREATE INDEX IF NOT EXISTS feedback_ratings_org_skill_idx
    ON feedback_ratings (org_id, skill_id, from_scope, to_scope)
    WHERE skill_id <> '';

-- Per-run lookup for the quality read path (was this turn rated, and how?).
CREATE INDEX IF NOT EXISTS feedback_ratings_run_idx
    ON feedback_ratings (org_id, run_id);

-- Quality-trend windows.
CREATE INDEX IF NOT EXISTS feedback_ratings_created_at_idx
    ON feedback_ratings (created_at);
