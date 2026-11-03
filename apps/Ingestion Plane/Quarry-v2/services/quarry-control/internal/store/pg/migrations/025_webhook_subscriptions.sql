-- INNH-5: tenant-owned webhook subscriptions.
--
-- Until now the webhooks table was global: no org column, and the fan-out listed every
-- active row. That was harmless only because no tenant route could create a row. This
-- migration makes a webhook an org resource so the org-scoped /v1/webhook-subscriptions API
-- (behind QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS) can sit on it, and so org erasure can purge it.
--
-- org_id '' marks a legacy row. The fan-out and the dispatcher both refuse such a row, so a
-- legacy subscription delivers to nobody (fail closed). Nothing here deletes it.
--
-- A delivery row holds the whole event payload (page URLs, fingerprints), so it also carries
-- org_id (for purge and tenant reads) and event_type (for the delivery listing), and a partial
-- index serves the retention sweep that deletes terminal rows past their window.
--
-- Idempotent: every statement is IF NOT EXISTS / IF EXISTS-safe.

ALTER TABLE webhooks ADD COLUMN IF NOT EXISTS org_id      TEXT NOT NULL DEFAULT '';
ALTER TABLE webhooks ADD COLUMN IF NOT EXISTS secret_hint TEXT NOT NULL DEFAULT '';
ALTER TABLE webhooks ADD COLUMN IF NOT EXISTS description TEXT NOT NULL DEFAULT '';
ALTER TABLE webhooks ADD COLUMN IF NOT EXISTS created_by  TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS webhooks_org_created_idx
    ON webhooks (org_id, created_at DESC, id);

ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS org_id     TEXT NOT NULL DEFAULT '';
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS event_type TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS webhook_deliveries_org_idx
    ON webhook_deliveries (org_id, webhook_id, created_at DESC)
    WHERE org_id <> '';

CREATE INDEX IF NOT EXISTS webhook_deliveries_terminal_idx
    ON webhook_deliveries (created_at)
    WHERE status IN ('success', 'failed', 'dlq');

-- A change can reach control by two paths (the edge's signed webhook and the run event batch);
-- dedupe_key makes the second delivery of one logical event a no-op per subscription.
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS dedupe_key TEXT NOT NULL DEFAULT '';

CREATE UNIQUE INDEX IF NOT EXISTS webhook_deliveries_dedupe_idx
    ON webhook_deliveries (webhook_id, dedupe_key)
    WHERE dedupe_key <> '';
