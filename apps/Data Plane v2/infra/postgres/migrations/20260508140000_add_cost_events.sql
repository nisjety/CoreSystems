-- Cost ledger persistence.
-- Embedding/rerank/extraction costs are published to NATS by the engines that
-- incur them; the orchestrator consumes those events and writes here so we
-- have a queryable history per org.

CREATE TABLE IF NOT EXISTS cost_events (
    id                BIGSERIAL    PRIMARY KEY,
    event_type        TEXT         NOT NULL,           -- 'embedding' | 'rerank' | 'extraction'
    model             TEXT         NOT NULL,
    org_id            TEXT         NOT NULL,
    count             INTEGER      NOT NULL DEFAULT 0,  -- units processed (chunks, queries, ...)
    estimated_tokens  BIGINT       NOT NULL DEFAULT 0,
    idempotency_key   TEXT,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- Idempotency: same event published twice (NATS at-least-once) collapses to
-- a single row.
CREATE UNIQUE INDEX IF NOT EXISTS uq_cost_events_idempotency
    ON cost_events (idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_cost_events_org_created
    ON cost_events (org_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_cost_events_type_created
    ON cost_events (event_type, created_at DESC);
