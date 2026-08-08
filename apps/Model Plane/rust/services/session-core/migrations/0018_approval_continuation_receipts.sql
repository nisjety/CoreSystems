-- Approval is authority, never evidence that work began. The delivery lease is
-- likewise not evidence: it is only permission for one worker to attempt a
-- continuation. These append-only records are the first durable execution
-- proof and must stay content-free.

CREATE TABLE IF NOT EXISTS approval_continuation_receipts (
    receipt_id TEXT PRIMARY KEY
        CHECK (length(receipt_id) BETWEEN 1 AND 128),
    delivery_id TEXT NOT NULL UNIQUE
        REFERENCES approval_delivery_outbox(delivery_id) ON DELETE CASCADE
        CHECK (length(delivery_id) BETWEEN 1 AND 128),
    approval_id TEXT NOT NULL
        REFERENCES approvals(id) ON DELETE CASCADE
        CHECK (length(approval_id) BETWEEN 1 AND 128),
    run_id TEXT NOT NULL
        REFERENCES runs(id)
        CHECK (length(run_id) BETWEEN 1 AND 128),
    org_id TEXT NOT NULL
        CHECK (length(org_id) BETWEEN 1 AND 128),
    user_id TEXT NOT NULL
        CHECK (length(user_id) BETWEEN 1 AND 128),
    descriptor_version SMALLINT NOT NULL CHECK (descriptor_version = 1),
    action_fingerprint TEXT NOT NULL
        CHECK (length(action_fingerprint) = 64),
    execution_service_id TEXT NOT NULL
        CHECK (length(execution_service_id) BETWEEN 1 AND 256),
    started_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS approval_continuation_receipts_run_idx
    ON approval_continuation_receipts (run_id, started_at DESC);

-- A final outcome is a separate immutable fact. It is never inferred from a
-- provider HTTP status, and it intentionally retains no provider payload.
CREATE TABLE IF NOT EXISTS approval_continuation_outcomes (
    receipt_id TEXT PRIMARY KEY
        REFERENCES approval_continuation_receipts(receipt_id) ON DELETE CASCADE,
    outcome TEXT NOT NULL
        CHECK (outcome IN ('completed', 'failed', 'cancelled')),
    provider_receipt_id TEXT
        CHECK (provider_receipt_id IS NULL OR length(provider_receipt_id) BETWEEN 1 AND 256),
    failure_code TEXT
        CHECK (failure_code IS NULL OR length(failure_code) BETWEEN 1 AND 64),
    finalized_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (
        (outcome = 'completed' AND provider_receipt_id IS NOT NULL AND failure_code IS NULL)
        OR
        (outcome IN ('failed', 'cancelled') AND provider_receipt_id IS NULL)
    )
);
