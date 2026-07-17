CREATE TABLE IF NOT EXISTS approval_delivery_outbox (
    delivery_id TEXT PRIMARY KEY
        CHECK (length(delivery_id) BETWEEN 1 AND 128),
    approval_id TEXT NOT NULL UNIQUE
        REFERENCES approvals(id) ON DELETE CASCADE
        CHECK (length(approval_id) BETWEEN 1 AND 128),
    run_id TEXT NOT NULL
        REFERENCES runs(id)
        CHECK (length(run_id) BETWEEN 1 AND 128),
    org_id TEXT NOT NULL
        CHECK (length(org_id) BETWEEN 1 AND 128),
    user_id TEXT NOT NULL
        CHECK (length(user_id) BETWEEN 1 AND 128),
    state TEXT NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'processing', 'terminal')),
    attempts INTEGER NOT NULL DEFAULT 0
        CHECK (attempts >= 0),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- A claim is owned by an authenticated delivery worker. The opaque token
    -- is stored only as a hash so a database read cannot replay the lease.
    lease_owner TEXT
        CHECK (lease_owner IS NULL OR length(lease_owner) BETWEEN 1 AND 256),
    lease_token_hash TEXT
        CHECK (lease_token_hash IS NULL OR length(lease_token_hash) = 64),
    lease_expires_at TIMESTAMPTZ,
    -- A bounded allowlisted classification only; no free-form values live here.
    last_failure_code TEXT
        CHECK (last_failure_code IS NULL OR length(last_failure_code) BETWEEN 1 AND 64),
    processing_at TIMESTAMPTZ,
    terminal_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (
        (lease_owner IS NULL AND lease_token_hash IS NULL AND lease_expires_at IS NULL)
        OR
        (lease_owner IS NOT NULL AND lease_token_hash IS NOT NULL AND lease_expires_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS approval_delivery_outbox_pending_idx
    ON approval_delivery_outbox (next_attempt_at, created_at)
    WHERE state = 'pending';

CREATE INDEX IF NOT EXISTS approval_delivery_outbox_expired_lease_idx
    ON approval_delivery_outbox (lease_expires_at, created_at)
    WHERE state = 'processing';
