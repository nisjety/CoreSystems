-- WS4.J: the ZDR run tombstone. A metadata-only record that a run happened under
-- Zero Data Retention, so an operator can answer "did this org run anything, when,
-- how did it end" without the run having left any content behind.
--
-- THE TABLE HOLDS NOTHING A CALLER TYPED OR A PAGE CONTAINED. No URL, no host, no
-- title, no query, no user id, no digest or fingerprint, no error text: the columns
-- below are ids, closed-vocabulary classes, counts and timestamps, and the CHECK
-- constraints refuse anything else at the database, whatever the writer sends. The
-- allowlist is the contract file crates/quarry-runtime/contracts/zdr-tombstone.schema.json;
-- resources/zdr_tombstone.go and the edge struct are tested to name exactly its keys.
--
-- Rows are immutable (the trigger refuses UPDATE) and short-lived (the tombstone
-- sweeper in internal/janitor/tombstones.go deletes by ended_at; org erasure deletes
-- by org_id). DELETE is deliberately allowed: deleting metadata is the privacy-safe
-- direction and nothing references a tombstone (no foreign key to any other table).
--
-- OFF BY DEFAULT. Nothing writes this table unless QUARRY_EDGE__ZDR_TOMBSTONE=1 on the
-- edge. Building it is not enabling it: counsel review gates enabling (ADR-4, plan
-- WS4.J), because a tombstone retains the fact that a ZDR run happened.
--
-- Idempotent (CREATE ... IF NOT EXISTS, CREATE OR REPLACE, DROP TRIGGER IF EXISTS before
-- CREATE TRIGGER), so a second application, or a late 015 on a database that already
-- holds 023, is a no-op. R-2 ledger: 015 is WS4's.

CREATE TABLE IF NOT EXISTS zdr_run_tombstones (
    run_id          TEXT PRIMARY KEY
        CONSTRAINT zdr_run_tombstones_run_id_shape CHECK (run_id ~ '^[A-Za-z0-9_-]{1,64}$'),
    org_id          TEXT NOT NULL
        CONSTRAINT zdr_run_tombstones_org_id_shape CHECK (org_id ~ '^[A-Za-z0-9_-]{1,64}$'),
    kind            TEXT NOT NULL
        CONSTRAINT zdr_run_tombstones_kind_closed
        CHECK (kind IN ('scrape', 'search', 'answer', 'extract', 'agent_run')),
    retention_class TEXT NOT NULL DEFAULT 'zdr'
        CONSTRAINT zdr_run_tombstones_zdr_only CHECK (retention_class = 'zdr'),
    outcome         TEXT NOT NULL
        CONSTRAINT zdr_run_tombstones_outcome_closed
        CHECK (outcome IN ('ok', 'error', 'refused', 'cancelled', 'timeout')),
    driver          TEXT
        CONSTRAINT zdr_run_tombstones_driver_closed
        CHECK (driver IS NULL OR driver IN ('static', 'browser', 'tls')),
    started_at      TIMESTAMPTZ,
    ended_at        TIMESTAMPTZ NOT NULL,
    step_count      INT
        CONSTRAINT zdr_run_tombstones_step_count_nonneg CHECK (step_count IS NULL OR step_count >= 0),
    cost_units      NUMERIC(18, 6)
        CONSTRAINT zdr_run_tombstones_cost_units_nonneg CHECK (cost_units IS NULL OR cost_units >= 0),
    error_class     TEXT
        CONSTRAINT zdr_run_tombstones_error_class_closed
        CHECK (error_class IS NULL OR error_class IN (
            'BAD_REQUEST', 'UNAUTHORIZED', 'FORBIDDEN', 'NOT_FOUND', 'CONFLICT',
            'RATE_LIMITED', 'TIMEOUT', 'SECURITY_BLOCKED', 'ACTION_UNKNOWN',
            'CHALLENGE_DETECTED', 'RUNTIME_NOT_READY', 'CHECKPOINT_LOST',
            'TARGET_REPAIR_REQUIRED', 'DRIVER_FAILED', 'UPSTREAM_BLOCKED',
            'UNSUPPORTED', 'UNSUPPORTED_MEDIA', 'BUDGET_EXHAUSTED', 'USER_TOOK_OVER',
            'INTERNAL', 'timeout', 'connect', 'status', 'decode', 'request')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS zdr_run_tombstones_org_ended_idx
    ON zdr_run_tombstones (org_id, ended_at DESC);

CREATE INDEX IF NOT EXISTS zdr_run_tombstones_ended_idx
    ON zdr_run_tombstones (ended_at);

-- Immutability: a tombstone is evidence that a run ended a certain way; it is never
-- corrected in place. (INSERT ... ON CONFLICT DO NOTHING, which the writer uses, never
-- reaches this trigger.)
CREATE OR REPLACE FUNCTION zdr_run_tombstones_refuse_update() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'zdr_run_tombstones rows are immutable'
        USING ERRCODE = 'restrict_violation';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS zdr_run_tombstones_immutable ON zdr_run_tombstones;
CREATE TRIGGER zdr_run_tombstones_immutable
    BEFORE UPDATE ON zdr_run_tombstones
    FOR EACH ROW EXECUTE FUNCTION zdr_run_tombstones_refuse_update();
