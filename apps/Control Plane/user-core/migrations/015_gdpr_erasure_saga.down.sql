DROP TABLE IF EXISTS user_erasure_operations;

ALTER TABLE user_audit_outbox
    DROP COLUMN IF EXISTS payload_purged_at;
