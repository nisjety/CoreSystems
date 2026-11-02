-- Identity-bound mentions of a human post and the Control-signed origin of a woken run
-- (build plan WS08-S2). Reversal: revert/0075_space_mentions_and_wake_origin.down.sql.
--
-- message_mentions: one row per (post, mentioned subject). Mentions exist only on a human post
-- (post_kind 'post'); agent output never carries them, so an agent cannot wake an agent. The
-- rows hold ids, kinds and enum codes, never text. ON DELETE CASCADE carries every existing
-- erase path (thread delete, GDPR purge) to them. `wake_expectation` is set exactly for agent
-- mentions: 'expected' = a wake may start one run for it; 'refused' = it was recorded but no run
-- may start, with `refusal_code` saying why.
CREATE TABLE IF NOT EXISTS message_mentions (
    org_id           TEXT        NOT NULL,
    message_id       TEXT        NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    thread_id        TEXT        NOT NULL,
    space_id         TEXT        NOT NULL,
    subject_id       TEXT        NOT NULL,
    subject_kind     TEXT        NOT NULL CHECK (subject_kind IN ('user', 'agent')),
    source           TEXT        NOT NULL DEFAULT 'typed' CHECK (source IN ('typed', 'locked')),
    wake_expectation TEXT        CHECK (wake_expectation IN ('expected', 'refused')),
    refusal_code     TEXT        CHECK (refusal_code IN ('respond_to_denied', 'mention_not_enabled', 'invocation_blocked', 'owner_only_thread')),
    ordinal          SMALLINT    NOT NULL DEFAULT 0,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (message_id, subject_id),
    CHECK ((subject_kind = 'agent') = (wake_expectation IS NOT NULL)),
    CHECK (refusal_code IS NULL OR wake_expectation = 'refused')
);

CREATE INDEX IF NOT EXISTS message_mentions_subject_idx
    ON message_mentions (org_id, space_id, subject_id, created_at DESC);

-- runs: the wake. `turn_origin` is the Control claim (NULL on every legacy run, read as
-- 'direct'); `trigger_message_id` + `agent_subject_id` name the post and the agent the run was
-- woken for. Added nullable, constraints NOT VALID then VALIDATE, so existing rows are checked
-- without a long lock.
ALTER TABLE runs
    ADD COLUMN IF NOT EXISTS turn_origin TEXT,
    ADD COLUMN IF NOT EXISTS trigger_message_id TEXT,
    ADD COLUMN IF NOT EXISTS agent_subject_id TEXT;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'runs_turn_origin_chk') THEN
        ALTER TABLE runs ADD CONSTRAINT runs_turn_origin_chk
            CHECK (turn_origin IS NULL OR turn_origin IN ('direct', 'ambient', 'automation')) NOT VALID;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'runs_wake_pair_chk') THEN
        ALTER TABLE runs ADD CONSTRAINT runs_wake_pair_chk
            CHECK ((trigger_message_id IS NULL) = (agent_subject_id IS NULL)) NOT VALID;
    END IF;
END
$$;

ALTER TABLE runs VALIDATE CONSTRAINT runs_turn_origin_chk;
ALTER TABLE runs VALIDATE CONSTRAINT runs_wake_pair_chk;

CREATE INDEX IF NOT EXISTS runs_trigger_idx
    ON runs (trigger_message_id, agent_subject_id)
    WHERE trigger_message_id IS NOT NULL;
