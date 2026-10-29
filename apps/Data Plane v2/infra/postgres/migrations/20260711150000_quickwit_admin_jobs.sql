CREATE TABLE IF NOT EXISTS quickwit_admin_jobs (
    job_id           TEXT        PRIMARY KEY,
    org_id           TEXT,
    scope_key        TEXT        GENERATED ALWAYS AS (COALESCE(org_id, '__global__')) STORED,
    global           BOOLEAN     NOT NULL DEFAULT FALSE,
    clear            BOOLEAN     NOT NULL DEFAULT FALSE,
    requested_by     TEXT        NOT NULL,
    approved_by      TEXT,
    approval_id      TEXT        NOT NULL,
    idempotency_key  TEXT        NOT NULL,
    reason           TEXT        NOT NULL,
    status           TEXT        NOT NULL DEFAULT 'requested'
                                 CHECK (status IN ('requested','approved','running','succeeded','failed')),
    checkpoint       SMALLINT    NOT NULL DEFAULT 0 CHECK (checkpoint BETWEEN 0 AND 6),
    batch_stage      SMALLINT    NOT NULL DEFAULT 0 CHECK (batch_stage BETWEEN 0 AND 6),
    batch_cursor     TEXT,
    attempts         INTEGER     NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    lease_owner      TEXT,
    lease_until      TIMESTAMPTZ,
    last_error       TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK ((global AND org_id IS NULL) OR (NOT global AND org_id IS NOT NULL)),
    CHECK (clear = FALSE),
    CHECK ((batch_stage = 0 AND batch_cursor IS NULL)
        OR (batch_stage > checkpoint AND batch_cursor IS NOT NULL AND length(batch_cursor) BETWEEN 1 AND 500)),
    CHECK (length(requested_by) BETWEEN 1 AND 200),
    CHECK (length(approval_id) BETWEEN 1 AND 200),
    CHECK (length(idempotency_key) BETWEEN 1 AND 200),
    CHECK (length(reason) BETWEEN 3 AND 500),
    CHECK (approved_by IS NULL OR approved_by <> requested_by),
    UNIQUE (scope_key, idempotency_key)
);

CREATE INDEX IF NOT EXISTS idx_quickwit_admin_jobs_claim
    ON quickwit_admin_jobs (created_at)
    WHERE status IN ('approved','running');

CREATE OR REPLACE FUNCTION protect_quickwit_admin_job_request()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.org_id IS DISTINCT FROM OLD.org_id
       OR NEW.global IS DISTINCT FROM OLD.global
       OR NEW.clear IS DISTINCT FROM OLD.clear
       OR NEW.requested_by IS DISTINCT FROM OLD.requested_by
       OR NEW.approval_id IS DISTINCT FROM OLD.approval_id
       OR NEW.idempotency_key IS DISTINCT FROM OLD.idempotency_key
       OR NEW.reason IS DISTINCT FROM OLD.reason THEN
        RAISE EXCEPTION 'quickwit admin job request fields are immutable';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS quickwit_admin_job_request_immutable ON quickwit_admin_jobs;
CREATE TRIGGER quickwit_admin_job_request_immutable
    BEFORE UPDATE ON quickwit_admin_jobs
    FOR EACH ROW EXECUTE FUNCTION protect_quickwit_admin_job_request();

CREATE TABLE IF NOT EXISTS quickwit_admin_job_audit (
    event_id     TEXT        PRIMARY KEY,
    job_id       TEXT        NOT NULL REFERENCES quickwit_admin_jobs(job_id),
    org_id       TEXT,
    actor        TEXT        NOT NULL,
    action       TEXT        NOT NULL,
    occurred_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_quickwit_admin_job_audit_job
    ON quickwit_admin_job_audit (job_id, occurred_at);

CREATE OR REPLACE FUNCTION reject_quickwit_admin_audit_mutation()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'quickwit admin audit is append-only';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS quickwit_admin_audit_immutable ON quickwit_admin_job_audit;
CREATE TRIGGER quickwit_admin_audit_immutable
    BEFORE UPDATE OR DELETE ON quickwit_admin_job_audit
    FOR EACH ROW EXECUTE FUNCTION reject_quickwit_admin_audit_mutation();
