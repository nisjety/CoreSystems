ALTER TABLE user_erasure_operations
    ADD COLUMN IF NOT EXISTS fanout_snapshot_at TIMESTAMPTZ;

CREATE TABLE IF NOT EXISTS user_erasure_fanout (
    child_event_id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL REFERENCES user_erasure_operations(operation_id) ON DELETE RESTRICT,
    org_id TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    processing_at TIMESTAMPTZ,
    published_at TIMESTAMPTZ,
    terminal_at TIMESTAMPTZ,
    last_error TEXT,
    requeue_count INTEGER NOT NULL DEFAULT 0 CHECK (requeue_count >= 0),
    requeued_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (length(child_event_id) BETWEEN 1 AND 128),
    CHECK (length(org_id) BETWEEN 1 AND 255),
    UNIQUE (operation_id, org_id)
);

CREATE INDEX IF NOT EXISTS user_erasure_fanout_pending_idx
    ON user_erasure_fanout (operation_id, next_attempt_at, created_at)
    WHERE published_at IS NULL AND terminal_at IS NULL;

-- Safely upgrade operations created by migration 015 before multi-org snapshots
-- existed. Their verified audit org is the only durable recipient still known.
INSERT INTO user_erasure_fanout (child_event_id, operation_id, org_id, published_at)
SELECT 'gdpr:fanout:legacy:' || md5(operation_id || ':' || org_id),
       operation_id,
       org_id,
       fanout_published_at
FROM user_erasure_operations
ON CONFLICT (operation_id, org_id) DO NOTHING;

UPDATE user_erasure_operations
SET fanout_snapshot_at = COALESCE(fanout_snapshot_at, created_at)
WHERE fanout_snapshot_at IS NULL;

ALTER TABLE user_audit_outbox
    ADD COLUMN IF NOT EXISTS requeue_count INTEGER NOT NULL DEFAULT 0 CHECK (requeue_count >= 0),
    ADD COLUMN IF NOT EXISTS requeued_at TIMESTAMPTZ;

COMMENT ON TABLE user_erasure_fanout IS
    'Immutable per-organization GDPR recipients captured before Auth/local cleanup; a child is complete only after a JetStream PubAck.';

REVOKE ALL ON TABLE user_erasure_fanout FROM PUBLIC;
