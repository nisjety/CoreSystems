-- 0008_routing_policy.sql
--
-- Verevon intent layer ("model router") runtime policy.
--
-- Layout choices:
--   * Lives in session-core's Postgres because session-core already owns the
--     durable run/checkpoint state the gateway and inference-core poll against.
--   * Singleton row: `id` is pinned to 1 with a CHECK so the table can hold at
--     most one policy. GetPolicy/SetPolicy always target `WHERE id = 1`.
--   * `config` is opaque JSONB — the canonical RoutingPolicy schema is owned by
--     inference-core. session-core only validates the wire string parses as JSON
--     before storing it; it never interprets the contents.
--   * `version` is a monotonic counter, server-incremented on every SetPolicy so
--     pollers can cheaply detect a change without diffing the JSON.
--   * Idempotent CREATE TABLE IF NOT EXISTS and seed INSERT ... ON CONFLICT so the
--     embedded migration runner can re-run safely.

CREATE TABLE IF NOT EXISTS routing_policy (
    id          INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    config      JSONB NOT NULL DEFAULT '{}',
    version     BIGINT NOT NULL DEFAULT 0,
    updated_by  TEXT NOT NULL DEFAULT '',
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed the singleton with the empty policy. GetPolicy returns this until the
-- first SetPolicy write, so the intent layer always has a row to read.
INSERT INTO routing_policy (id, config) VALUES (1, '{}') ON CONFLICT (id) DO NOTHING;
