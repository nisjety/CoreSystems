ALTER TABLE user_audit_outbox
    DROP COLUMN IF EXISTS requeued_at,
    DROP COLUMN IF EXISTS requeue_count;

DROP TABLE IF EXISTS user_erasure_fanout;

ALTER TABLE user_erasure_operations
    DROP COLUMN IF EXISTS fanout_snapshot_at;
