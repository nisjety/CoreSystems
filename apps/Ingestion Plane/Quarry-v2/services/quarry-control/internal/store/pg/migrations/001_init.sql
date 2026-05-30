-- Quarry V2 control plane — initial schema.
-- Phase 1.1. Keeps DB interface shape identical to in-memory backend.

CREATE TABLE IF NOT EXISTS jobs (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,
    status      TEXT NOT NULL,
    policy      JSONB NOT NULL,
    params      JSONB,
    created_at  BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS jobs_kind_status_created_idx
    ON jobs (kind, status, created_at DESC);

CREATE TABLE IF NOT EXISTS stores (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL,
    created_at  BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS snapshots (
    id          TEXT PRIMARY KEY,
    run_id      TEXT NOT NULL,
    bucket      TEXT NOT NULL,
    created_at  BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS snapshots_run_id_idx ON snapshots (run_id);

CREATE TABLE IF NOT EXISTS artifacts (
    id          TEXT PRIMARY KEY,
    run_id      TEXT NOT NULL,
    kind        TEXT NOT NULL,
    key         TEXT NOT NULL,
    bytes       BIGINT NOT NULL,
    created_at  BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS artifacts_run_id_idx ON artifacts (run_id);

CREATE TABLE IF NOT EXISTS profiles (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    snapshot_uri  TEXT NOT NULL,
    created_at    BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS schedules (
    id           TEXT PRIMARY KEY,
    cron         TEXT NOT NULL,
    target_kind  TEXT NOT NULL,
    target_ref   TEXT NOT NULL,
    enabled      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS events (
    event_id        TEXT PRIMARY KEY,
    run_id          TEXT,
    job_id          TEXT,
    type            TEXT NOT NULL,
    ts              TIMESTAMPTZ NOT NULL,
    seq             BIGINT NOT NULL,
    payload         JSONB,
    idempotency_key TEXT
);
-- Contract §1.3: unique (run_id, seq) to prevent dupes.
CREATE UNIQUE INDEX IF NOT EXISTS events_run_seq_uidx
    ON events (run_id, seq) WHERE run_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS events_job_id_idx ON events (job_id) WHERE job_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS events_idem_idx ON events (idempotency_key) WHERE idempotency_key IS NOT NULL;
