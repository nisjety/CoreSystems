-- WS05-S4: keyset-paginated audit list reads.
--
-- Additive: three indexes, nothing on audit_events changes. They back
-- GET /v1/audit, which pages on (ingested_at DESC, id DESC) within one org and
-- optionally narrows by user_id or plane. IF NOT EXISTS keeps a re-apply safe.
-- The revert is migrations/revert/011_audit_read_indexes.down.sql.

CREATE INDEX IF NOT EXISTS idx_audit_events_org_ts_id
    ON audit_events (org_id, ingested_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_audit_events_org_user_ts
    ON audit_events (org_id, user_id, ingested_at DESC);

CREATE INDEX IF NOT EXISTS idx_audit_events_org_plane_ts
    ON audit_events (org_id, plane, ingested_at DESC);
