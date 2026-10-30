-- Layer 3 (vsv2-interactive): append-only audit of every write attempt.
CREATE TABLE IF NOT EXISTS vsv2.write_audit (
    audit_id      UUID PRIMARY KEY,
    plan_id       UUID,
    tenant_id     TEXT NOT NULL,
    operation     TEXT NOT NULL,
    caller        TEXT NOT NULL,
    stage         TEXT NOT NULL CHECK (stage IN ('dry_run','denied','executing','succeeded','failed','unknown')),
    target        TEXT,
    method        TEXT,
    path          TEXT,
    payload_hash  TEXT,
    payload       JSONB,
    http_status   INTEGER,
    detail        TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS write_audit_plan_idx ON vsv2.write_audit (plan_id);
-- One execution claim per plan, enforced by the database (closes the check-then-send race).
CREATE UNIQUE INDEX IF NOT EXISTS write_audit_one_claim_per_plan
    ON vsv2.write_audit (plan_id) WHERE stage = 'executing';
CREATE INDEX IF NOT EXISTS write_audit_tenant_created_idx ON vsv2.write_audit (tenant_id, created_at DESC);

CREATE OR REPLACE FUNCTION vsv2.write_audit_immutable() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'vsv2.write_audit is append-only';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS write_audit_no_update ON vsv2.write_audit;
CREATE TRIGGER write_audit_no_update
    BEFORE UPDATE OR DELETE ON vsv2.write_audit
    FOR EACH ROW EXECUTE FUNCTION vsv2.write_audit_immutable();

DROP TRIGGER IF EXISTS write_audit_no_truncate ON vsv2.write_audit;
CREATE TRIGGER write_audit_no_truncate
    BEFORE TRUNCATE ON vsv2.write_audit
    FOR EACH STATEMENT EXECUTE FUNCTION vsv2.write_audit_immutable();
