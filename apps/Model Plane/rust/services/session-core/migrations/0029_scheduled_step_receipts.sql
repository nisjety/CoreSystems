-- Service-owned scheduled-step claim/receipt ledger.
-- The table is deliberately metadata-only: no goal, tool input, provider
-- output, bearer, or model content is retained here.

CREATE TABLE IF NOT EXISTS scheduled_step_receipts (
    run_id          TEXT NOT NULL REFERENCES runs(id),
    step_id         TEXT NOT NULL,
    org_id          TEXT NOT NULL,
    thread_id       TEXT NOT NULL REFERENCES threads(id),
    space_id        TEXT NOT NULL,
    schedule_id     TEXT NOT NULL,
    fire_key        TEXT NOT NULL,
    template_digest TEXT NOT NULL,
    step_index      BIGINT NOT NULL CHECK (step_index >= 0),
    policy_digest   TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    receipt_id      TEXT NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('claimed', 'completed', 'failed', 'unknown_outcome')),
    output_digest   TEXT NOT NULL DEFAULT '',
    error_code      TEXT NOT NULL DEFAULT '',
    unknown_outcome BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (run_id, step_id),
    UNIQUE (receipt_id),
    UNIQUE (org_id, idempotency_key)
);

CREATE INDEX IF NOT EXISTS idx_scheduled_step_receipts_run
    ON scheduled_step_receipts (run_id, step_index);
