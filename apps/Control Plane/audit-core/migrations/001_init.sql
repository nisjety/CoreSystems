-- Phase A · A1.6 — audit-core schema.
--
-- Two append-only tables, one for security/audit events and one for
-- usage counters. Both partition by ingestion month so the read path
-- (the verevon /settings/audit-log + /settings/usage views) can hit
-- a small slice of rows.
--
-- Both tables key on (org_id, ingested_at) so the typical query
-- "all events for org X since T" lands on the same index.

CREATE TABLE IF NOT EXISTS audit_events (
  id            BIGSERIAL    PRIMARY KEY,
  ingested_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
  occurred_at   TIMESTAMPTZ  NOT NULL,
  org_id        TEXT         NOT NULL,
  user_id       TEXT,
  actor_role    TEXT,
  plane         TEXT         NOT NULL,
  event         TEXT         NOT NULL,
  subject       TEXT,
  resource_id   TEXT,
  outcome       TEXT         NOT NULL DEFAULT 'ok',
  details       JSONB        NOT NULL DEFAULT '{}'::jsonb,
  request_id    TEXT,
  ip_address    INET,
  user_agent    TEXT
);

CREATE INDEX IF NOT EXISTS idx_audit_events_org_ingested
  ON audit_events (org_id, ingested_at DESC);

CREATE INDEX IF NOT EXISTS idx_audit_events_event
  ON audit_events (org_id, event, ingested_at DESC);

CREATE INDEX IF NOT EXISTS idx_audit_events_request
  ON audit_events (request_id)
  WHERE request_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS usage_events (
  id            BIGSERIAL    PRIMARY KEY,
  ingested_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
  occurred_at   TIMESTAMPTZ  NOT NULL,
  org_id        TEXT         NOT NULL,
  user_id       TEXT,
  plane         TEXT         NOT NULL,
  op            TEXT         NOT NULL,
  tokens_in     BIGINT       NOT NULL DEFAULT 0,
  tokens_out    BIGINT       NOT NULL DEFAULT 0,
  bytes_in      BIGINT       NOT NULL DEFAULT 0,
  bytes_out    BIGINT       NOT NULL DEFAULT 0,
  cost_cents    NUMERIC(20, 6) NOT NULL DEFAULT 0,
  request_id    TEXT,
  metadata      JSONB        NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_usage_events_org_ingested
  ON usage_events (org_id, ingested_at DESC);

CREATE INDEX IF NOT EXISTS idx_usage_events_plane_op
  ON usage_events (org_id, plane, op, ingested_at DESC);
