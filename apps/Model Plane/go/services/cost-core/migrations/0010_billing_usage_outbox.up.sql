CREATE TABLE IF NOT EXISTS cost_billing_outbox (
 entry_id UUID PRIMARY KEY REFERENCES cost_entries(id) ON DELETE CASCADE,
 attempts INTEGER NOT NULL DEFAULT 0,
 next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 delivered_at TIMESTAMPTZ,
 last_status INTEGER,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS cost_billing_outbox_pending ON cost_billing_outbox(next_attempt_at) WHERE delivered_at IS NULL;
-- No historical backfill: old records may already have been billed elsewhere.
