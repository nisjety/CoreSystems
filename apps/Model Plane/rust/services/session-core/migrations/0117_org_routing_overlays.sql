-- 0117: the per-organisation routing overlay (W4-BYO-3, EP-13 D6, F-t3-065).
--
-- One row per organisation. `config_json` is the organisation's narrowing of the platform model catalogue and
-- role table, as `{"hidden": [...], "order": [...], "roles": {"<purpose>": ["<model id>", ...]}}`:
--
--   hidden  model ids the organisation's listing drops and its pinned requests refuse
--   order   model ids the organisation's listing puts first
--   roles   per side-task purpose, up to four candidate model ids the `verevon-role` resolver tries before the
--           platform role table
--
-- The overlay can only narrow. It has no aliases and nothing in it adds a model: inference-core drops any id the
-- platform catalogue does not list, and every role candidate is still gated by the caller's entitlement and the
-- ZDR, privacy-tier and residency floors. session-core validates the shape and the size and stores the document;
-- inference-core reads it through the RoutingPolicy service and applies it.
--
--   version     server-incremented on every write; a write that names the version it read is refused when the
--               row has moved on
--   updated_by  the identity that last wrote the row
--
-- Zero Data Retention: configuration and identities only, no org or user content. A ZDR caller's write is refused
-- by the RPC before this table is touched. Rows are erased with the organisation (gdpr::purge_org_data).
--
-- Additive: one new table, no backfill, nothing reads it while SESSION_ORG_ROUTING_OVERLAY is off. The reversal
-- is in revert/0117_org_routing_overlays.down.sql.
CREATE TABLE IF NOT EXISTS org_routing_overlays (
    org_id TEXT PRIMARY KEY,
    config_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    version BIGINT NOT NULL DEFAULT 1,
    updated_by TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
