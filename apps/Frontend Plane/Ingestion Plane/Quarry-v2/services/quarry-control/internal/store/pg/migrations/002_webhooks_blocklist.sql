-- Quarry V2 control plane — webhooks, webhook deliveries, blocklist.
-- Phase 1A: Go control-plane parity. Column names match the Go structs in
-- services/quarry-control/internal/store/store.go exactly so pg/resources.go
-- can scan rows without aliasing.

CREATE TABLE IF NOT EXISTS webhooks (
    id          TEXT PRIMARY KEY,
    url         TEXT NOT NULL,
    secret      TEXT NOT NULL,
    events      TEXT[] NOT NULL DEFAULT '{}',
    active      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS webhooks_created_idx
    ON webhooks (created_at DESC, id);

CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id              TEXT PRIMARY KEY,
    webhook_id      TEXT NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    event_id        TEXT NOT NULL,
    attempt         INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL,
    last_error      TEXT NOT NULL DEFAULT '',
    next_attempt_at BIGINT NOT NULL DEFAULT 0,
    created_at      BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS webhook_deliveries_status_next_idx
    ON webhook_deliveries (status, next_attempt_at);
CREATE INDEX IF NOT EXISTS webhook_deliveries_webhook_idx
    ON webhook_deliveries (webhook_id, created_at DESC);
CREATE INDEX IF NOT EXISTS webhook_deliveries_created_idx
    ON webhook_deliveries (created_at DESC, id);

CREATE TABLE IF NOT EXISTS blocklist_entries (
    id          TEXT PRIMARY KEY,
    pattern     TEXT NOT NULL,
    is_regex    BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS blocklist_entries_created_idx
    ON blocklist_entries (created_at DESC, id);
