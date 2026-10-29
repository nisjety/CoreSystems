-- cost-core durable ledger.
--
-- One row per cost-bearing event (a model invocation, a tool call, etc.).
-- Aggregation (per-run / per-org / per-user totals) is derived from this
-- append-only table at query time so we never lose the underlying detail and
-- can always re-aggregate. The gateway's budget guard reads the rolled-up
-- totals; runs/billing read both detail and rollups.
CREATE TABLE IF NOT EXISTS cost_entries (
    id              uuid        PRIMARY KEY,
    org_id          text        NOT NULL,
    user_id         text        NOT NULL DEFAULT '',
    run_id          text        NOT NULL DEFAULT '',
    request_id      text        NOT NULL DEFAULT '',
    model           text        NOT NULL DEFAULT '',
    input_tokens    bigint      NOT NULL DEFAULT 0,
    output_tokens   bigint      NOT NULL DEFAULT 0,
    cost_usd        numeric(20, 10) NOT NULL DEFAULT 0,
    -- Idempotency key from the producing envelope. When present and non-empty
    -- it dedupes retries: the same event recorded twice is stored once.
    idempotency_key text        NOT NULL DEFAULT '',
    metadata        jsonb       NOT NULL DEFAULT '{}'::jsonb,
    created_at      timestamptz NOT NULL DEFAULT now()
);

-- Dedupe on non-empty idempotency keys. Empty keys (legacy / fire-and-forget
-- events) are allowed to repeat — NULLS NOT DISTINCT is not used because empty
-- string is a real value, so we restrict the unique index to non-empty keys.
CREATE UNIQUE INDEX IF NOT EXISTS cost_entries_idempotency_key_unique
    ON cost_entries (idempotency_key)
    WHERE idempotency_key <> '';

-- Hot path: budget guard rolls up by (org_id) and (org_id, user_id).
CREATE INDEX IF NOT EXISTS cost_entries_org_idx
    ON cost_entries (org_id);

CREATE INDEX IF NOT EXISTS cost_entries_org_user_idx
    ON cost_entries (org_id, user_id);

-- Per-run rollups for the runs/budget surfaces.
CREATE INDEX IF NOT EXISTS cost_entries_run_idx
    ON cost_entries (run_id)
    WHERE run_id <> '';

-- Time-range aggregation / listing.
CREATE INDEX IF NOT EXISTS cost_entries_created_at_idx
    ON cost_entries (created_at);
