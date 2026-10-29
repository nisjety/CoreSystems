CREATE TABLE IF NOT EXISTS billing_retry_jobs (
    id BIGSERIAL PRIMARY KEY,
    kind TEXT NOT NULL,
    dedupe_key TEXT NOT NULL UNIQUE,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_billing_retry_jobs_due
    ON billing_retry_jobs (status, next_attempt_at);
