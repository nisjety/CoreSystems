-- Cycle 23 parity: org-scoped list families.
--
-- snapshots_v2 is the enriched snapshot read model consumed by
-- GET /v1/team/{org}/snapshots/v2 (quarry_core::resources::Snapshot).
-- The legacy snapshots table stays untouched; backfill is best-effort so the
-- table is usable immediately after apply.
--
-- quarry_request_queues gains the columns required by the upgraded
-- RequestQueueSummary wire shape (kind/status/stats). Existing rows default
-- to status='active' so no data migration is needed.

CREATE TABLE IF NOT EXISTS snapshots_v2 (
    id           BIGSERIAL PRIMARY KEY,
    snapshot_id  TEXT        NOT NULL,
    org_id       TEXT        NOT NULL,
    source_id    TEXT        NOT NULL DEFAULT '',
    url          TEXT        NOT NULL DEFAULT '',
    title        TEXT        NOT NULL DEFAULT '',
    content_hash TEXT        NOT NULL DEFAULT '',
    byte_size    BIGINT      NOT NULL DEFAULT 0,
    status       TEXT        NOT NULL DEFAULT 'complete',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, snapshot_id)
);

CREATE INDEX IF NOT EXISTS idx_snapshots_v2_org_created
    ON snapshots_v2 (org_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_snapshots_v2_org_source_created
    ON snapshots_v2 (org_id, source_id, created_at DESC)
    WHERE source_id <> '';

-- Best-effort backfill from the legacy snapshots table. The legacy table is
-- owned by other services and its exact shape varies, so any mismatch
-- (missing table or columns) is swallowed: parity handlers work fine with
-- an empty read model until writers populate it.
DO $$
BEGIN
    IF to_regclass('public.snapshots') IS NOT NULL THEN
        BEGIN
            INSERT INTO snapshots_v2 (snapshot_id, org_id, source_id, url, title, content_hash, byte_size, status, created_at)
            SELECT s.snapshot_id, s.org_id,
                   COALESCE(s.source_id, ''),
                   COALESCE(s.url, ''),
                   COALESCE(s.title, ''),
                   COALESCE(s.content_hash, s.hash, ''),
                   COALESCE(s.byte_size, s.size_bytes, 0),
                   COALESCE(s.status, 'complete'),
                   COALESCE(s.created_at, now())
            FROM snapshots s
            ON CONFLICT (org_id, snapshot_id) DO NOTHING;
        EXCEPTION WHEN OTHERS THEN
            RAISE NOTICE 'snapshots_v2 backfill skipped: %', SQLERRM;
        END;
    END IF;
END $$;

ALTER TABLE quarry_request_queues
    ADD COLUMN IF NOT EXISTS kind       TEXT NOT NULL DEFAULT 'crawl',
    ADD COLUMN IF NOT EXISTS status     TEXT NOT NULL DEFAULT 'active',
    ADD COLUMN IF NOT EXISTS acked      BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS failed     BIGINT NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_request_queues_org_status_created
    ON quarry_request_queues (org_id, status, created_at DESC, id DESC);
