-- session-core 0002: event envelope hydration + deterministic ordinal columns
--
-- Goals:
--   1. Carry the full proto Event envelope on the events table (org_id, user_id,
--      correlation/causation/idempotency/resource_ref/type_url/producer/schema_version)
--      so replay_thread can reconstruct a canonical pb::Event instead of empty strings.
--   2. Replace CTE-based MAX+1 counting (visibility-ambiguous) with trigger-assigned
--      monotonic ordinals on checkpoints and on STEP_COMPLETED/ACTION_COMPLETED events.
--   3. Add runs.ended_at (already referenced by grpc.rs complete_step UPDATE).
--   4. Enforce idempotency via unique (org_id, idempotency_key) when the key is set.
--
-- Concurrency note: triggers use MAX+1 per run_id. session-core is the single writer
-- for a given run (execution-core submits STEP_COMPLETED/CHECKPOINT_SAVED sequentially
-- per run), so the MAX+1 race is not observable in practice. If multi-writer semantics
-- are ever introduced, wrap the trigger body in SELECT ... FOR UPDATE on runs.

-- -------------------------------------------------------------------------
-- runs: add ended_at (referenced but absent in 0001)
-- -------------------------------------------------------------------------
ALTER TABLE runs ADD COLUMN IF NOT EXISTS ended_at TIMESTAMPTZ;

-- -------------------------------------------------------------------------
-- events: proto Event envelope hydration columns
-- -------------------------------------------------------------------------
ALTER TABLE events ADD COLUMN IF NOT EXISTS org_id          TEXT NOT NULL DEFAULT '';
ALTER TABLE events ADD COLUMN IF NOT EXISTS user_id         TEXT NOT NULL DEFAULT '';
ALTER TABLE events ADD COLUMN IF NOT EXISTS correlation_id  TEXT NOT NULL DEFAULT '';
ALTER TABLE events ADD COLUMN IF NOT EXISTS causation_id    TEXT NOT NULL DEFAULT '';
ALTER TABLE events ADD COLUMN IF NOT EXISTS idempotency_key TEXT NOT NULL DEFAULT '';
ALTER TABLE events ADD COLUMN IF NOT EXISTS resource_ref    TEXT NOT NULL DEFAULT '';
ALTER TABLE events ADD COLUMN IF NOT EXISTS type_url        TEXT NOT NULL DEFAULT '';
ALTER TABLE events ADD COLUMN IF NOT EXISTS producer        TEXT NOT NULL DEFAULT 'session-core';
ALTER TABLE events ADD COLUMN IF NOT EXISTS schema_version  INTEGER NOT NULL DEFAULT 1;

-- Strong idempotency: when idempotency_key is set, it must be unique per org.
CREATE UNIQUE INDEX IF NOT EXISTS idx_events_idempotency_key
    ON events (org_id, idempotency_key)
    WHERE idempotency_key <> '';

-- Deterministic replay order index (ts, id) already exists from 0001 as idx_events_run_id.
-- Add a query-friendly index for org-scoped lookups.
CREATE INDEX IF NOT EXISTS idx_events_org_ts ON events (org_id, ts) WHERE org_id <> '';

-- -------------------------------------------------------------------------
-- events.step_ordinal: monotonic per run for STEP_COMPLETED / ACTION_COMPLETED
-- -------------------------------------------------------------------------
ALTER TABLE events ADD COLUMN IF NOT EXISTS step_ordinal BIGINT;

CREATE OR REPLACE FUNCTION assign_event_step_ordinal() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.event_type IN ('STEP_COMPLETED', 'ACTION_COMPLETED')
       AND NEW.step_ordinal IS NULL THEN
        SELECT COALESCE(MAX(step_ordinal), 0) + 1
          INTO NEW.step_ordinal
          FROM events
         WHERE run_id = NEW.run_id
           AND event_type IN ('STEP_COMPLETED', 'ACTION_COMPLETED');
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_events_step_ordinal ON events;
CREATE TRIGGER trg_events_step_ordinal
    BEFORE INSERT ON events
    FOR EACH ROW
    EXECUTE FUNCTION assign_event_step_ordinal();

CREATE UNIQUE INDEX IF NOT EXISTS idx_events_run_step_ordinal
    ON events (run_id, step_ordinal)
    WHERE step_ordinal IS NOT NULL;

-- -------------------------------------------------------------------------
-- checkpoints.ordinal: monotonic per run
-- -------------------------------------------------------------------------
ALTER TABLE checkpoints ADD COLUMN IF NOT EXISTS ordinal BIGINT;

CREATE OR REPLACE FUNCTION assign_checkpoint_ordinal() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.ordinal IS NULL THEN
        SELECT COALESCE(MAX(ordinal), 0) + 1
          INTO NEW.ordinal
          FROM checkpoints
         WHERE run_id = NEW.run_id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_checkpoints_ordinal ON checkpoints;
CREATE TRIGGER trg_checkpoints_ordinal
    BEFORE INSERT ON checkpoints
    FOR EACH ROW
    EXECUTE FUNCTION assign_checkpoint_ordinal();

CREATE UNIQUE INDEX IF NOT EXISTS idx_checkpoints_run_ordinal
    ON checkpoints (run_id, ordinal)
    WHERE ordinal IS NOT NULL;
