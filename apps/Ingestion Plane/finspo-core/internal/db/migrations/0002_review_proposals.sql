-- finspo-core Phase 4 — review proposals + supporting indexes for analytics.
--
-- review_proposals is a mutable state machine; audit_log (already in 0001)
-- remains the append-only history of state transitions.

CREATE TABLE IF NOT EXISTS review_proposals (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id TEXT        NOT NULL,
    proposed_by     TEXT        NOT NULL,
    kind            TEXT        NOT NULL,   -- 'delete' | 'archive'
    reason          TEXT        NOT NULL,
    item_pks        UUID[]      NOT NULL,
    status          TEXT        NOT NULL DEFAULT 'pending',
                    -- pending | approved | rejected | executed | failed
    decided_by      TEXT,
    decided_at      TIMESTAMPTZ,
    executed_at     TIMESTAMPTZ,
    failure_reason  TEXT,
    notes           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (kind IN ('delete', 'archive')),
    CHECK (status IN ('pending', 'approved', 'rejected', 'executed', 'failed')),
    CHECK (array_length(item_pks, 1) > 0)
);

CREATE INDEX IF NOT EXISTS idx_review_proposals_org_status_created
    ON review_proposals (organization_id, status, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_review_proposals_kind
    ON review_proposals (kind);

DROP TRIGGER IF EXISTS trg_review_proposals_updated_at ON review_proposals;
CREATE TRIGGER trg_review_proposals_updated_at
    BEFORE UPDATE ON review_proposals
    FOR EACH ROW EXECUTE PROCEDURE finspo_set_updated_at();

-- ---------------------------------------------------------------------------
-- Analytics support indexes for items.
--
-- (idx_items_quick_xor_hash + idx_items_sha1_hash already exist from 0001;
--  this migration adds compound indexes that match the analytics queries.)
-- ---------------------------------------------------------------------------

-- "largest live files per org": speeds up ORDER BY size_bytes DESC scans.
CREATE INDEX IF NOT EXISTS idx_items_org_size_live
    ON items (organization_id, size_bytes DESC)
 WHERE deleted_at IS NULL AND is_folder = FALSE;

-- "inactive live files per org": speeds up filtering by modified_at < cutoff.
CREATE INDEX IF NOT EXISTS idx_items_org_modified_live
    ON items (organization_id, modified_at)
 WHERE deleted_at IS NULL AND is_folder = FALSE;
