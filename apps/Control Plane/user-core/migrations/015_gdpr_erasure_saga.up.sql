ALTER TABLE user_audit_outbox
    ADD COLUMN IF NOT EXISTS payload_purged_at TIMESTAMPTZ;

CREATE TABLE IF NOT EXISTS user_erasure_operations (
    operation_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('hard_delete', 'anonymize')),
    actor_id TEXT NOT NULL,
    actor_role TEXT NOT NULL CHECK (actor_role IN ('self', 'admin')),
    org_id TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    processing_at TIMESTAMPTZ,
    auth_completed_at TIMESTAMPTZ,
    local_completed_at TIMESTAMPTZ,
    local_user_deleted BOOLEAN NOT NULL DEFAULT false,
    audit_enqueued_at TIMESTAMPTZ,
    fanout_published_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (length(operation_id) BETWEEN 1 AND 128),
    CHECK (length(user_id) BETWEEN 1 AND 255),
    CHECK (length(actor_id) BETWEEN 1 AND 255),
    CHECK (length(org_id) BETWEEN 1 AND 255),
    UNIQUE (user_id, mode)
);

CREATE INDEX IF NOT EXISTS user_erasure_operations_pending_idx
    ON user_erasure_operations (next_attempt_at, created_at)
    WHERE completed_at IS NULL;

COMMENT ON TABLE user_erasure_operations IS
    'Minimal durable GDPR erasure saga state. Raw Auth receipts and erasure payloads are never retained here.';

REVOKE ALL ON TABLE user_erasure_operations FROM PUBLIC;
