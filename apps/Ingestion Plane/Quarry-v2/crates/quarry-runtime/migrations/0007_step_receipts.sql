-- Durable, tenant-bound proof for every agent action.
CREATE TABLE IF NOT EXISTS quarry_step_receipts (
    receipt_id      TEXT        PRIMARY KEY,
    run_id          TEXT        NOT NULL,
    org_id          TEXT        NOT NULL CHECK (org_id <> ''),
    step            INTEGER     NOT NULL CHECK (step >= 0),
    started_at      TIMESTAMPTZ NOT NULL,
    finished_at     TIMESTAMPTZ NOT NULL,
    action          JSONB       NOT NULL,
    outcome         JSONB       NOT NULL,
    observation     JSONB,
    correction_of   TEXT,
    cost_micro_usd  BIGINT      NOT NULL CHECK (cost_micro_usd >= 0)
);

CREATE INDEX IF NOT EXISTS quarry_step_receipts_org_run_idx
    ON quarry_step_receipts (org_id, run_id, step ASC, finished_at ASC);
