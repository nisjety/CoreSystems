-- ============================================================
--  Data Plane — Postgres Schema
--  Run automatically on first container start via init.sql
-- ============================================================

CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- ── documents ────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS documents (
    document_id   TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id        TEXT         NOT NULL,
    source        TEXT         NOT NULL,   -- sharepoint | web | pdf | notion | manual
    type          TEXT         NOT NULL,   -- policy | invoice | handbook | ...
    title         TEXT         NOT NULL,
    content       TEXT         NOT NULL,
    status        TEXT         NOT NULL DEFAULT 'pending',
                                           -- pending | processing | indexed | failed
    metadata      JSONB        NOT NULL DEFAULT '{}',
    error_message TEXT,
    created_by    TEXT,                     -- user_id of the uploader (GDPR / audit)
    deleted_by    TEXT,                     -- user_id who triggered deletion
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_documents_org_id     ON documents (org_id);
CREATE INDEX IF NOT EXISTS idx_documents_status     ON documents (status);
CREATE INDEX IF NOT EXISTS idx_documents_org_status ON documents (org_id, status);
CREATE INDEX IF NOT EXISTS idx_documents_type       ON documents (org_id, type);
-- Deduplication index: prevents inserting the same crawled URL twice for the same org.
-- Used with ON CONFLICT upsert in create_document when source = 'quarry'.
CREATE UNIQUE INDEX IF NOT EXISTS idx_documents_crawl_url_dedup
    ON documents (org_id, (metadata->>'url'))
    WHERE source = 'quarry';

-- ── knowledge_units ──────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS knowledge_units (
    knowledge_id      TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    document_id       TEXT         NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    org_id            TEXT         NOT NULL,
    chunk_index       INTEGER      NOT NULL,
    text              TEXT         NOT NULL,
    embedding_status  TEXT         NOT NULL DEFAULT 'pending',
                                             -- pending | done | failed
    metadata          JSONB        NOT NULL DEFAULT '{}',
    error_message     TEXT,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ku_document_id      ON knowledge_units (document_id);
CREATE INDEX IF NOT EXISTS idx_ku_org_id           ON knowledge_units (org_id);
CREATE INDEX IF NOT EXISTS idx_ku_embedding_status ON knowledge_units (embedding_status);

-- ── helper: auto-update updated_at ───────────────────────────────────────────

CREATE OR REPLACE FUNCTION trigger_set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS set_documents_updated_at ON documents;
CREATE TRIGGER set_documents_updated_at
    BEFORE UPDATE ON documents
    FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();

DROP TRIGGER IF EXISTS set_knowledge_units_updated_at ON knowledge_units;
CREATE TRIGGER set_knowledge_units_updated_at
    BEFORE UPDATE ON knowledge_units
    FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();
-- ── org_quotas ──────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS org_quotas (
    org_id                TEXT         PRIMARY KEY,
    plan_tier             TEXT         NOT NULL DEFAULT 'free',
                                                  -- free | professional | enterprise
    documents_limit       INTEGER      NOT NULL DEFAULT 100,
    api_calls_per_month   INTEGER      NOT NULL DEFAULT 10000,
    storage_gb            DECIMAL      NOT NULL DEFAULT 1.0,
    concurrent_users      INTEGER      NOT NULL DEFAULT 1,
    custom_models         BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at            TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at            TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_org_quotas_plan_tier ON org_quotas (plan_tier);

DROP TRIGGER IF EXISTS set_org_quotas_updated_at ON org_quotas;
CREATE TRIGGER set_org_quotas_updated_at
    BEFORE UPDATE ON org_quotas
    FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();

-- ── user_quota_overrides ────────────────────────────────────────────────────
-- Per-user upgrades within an org. Effective limit = max(org, user_override).

CREATE TABLE IF NOT EXISTS user_quota_overrides (
    user_id             TEXT         NOT NULL,
    org_id              TEXT         NOT NULL,
    plan_tier           TEXT         NOT NULL DEFAULT 'free',
    documents_limit     INTEGER      NOT NULL DEFAULT 100,
    api_calls_per_month INTEGER      NOT NULL DEFAULT 10000,
    storage_gb          DECIMAL      NOT NULL DEFAULT 1.0,
    custom_models       BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at          TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, org_id)
);

DROP TRIGGER IF EXISTS set_user_quota_overrides_updated_at ON user_quota_overrides;
CREATE TRIGGER set_user_quota_overrides_updated_at
    BEFORE UPDATE ON user_quota_overrides
    FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();

-- ── data_plane_audit_log ────────────────────────────────────────────────────
-- Append-only GDPR audit trail for all data access.

CREATE TABLE IF NOT EXISTS data_plane_audit_log (
    id             BIGSERIAL    PRIMARY KEY,
    user_id        TEXT         NOT NULL,
    org_id         TEXT         NOT NULL,
    action         TEXT         NOT NULL,   -- create | read | delete | search | purge
    resource_type  TEXT         NOT NULL,   -- document | knowledge_unit | search
    resource_id    TEXT,
    ip_address     TEXT,
    user_agent     TEXT,
    details        TEXT,
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_user_id    ON data_plane_audit_log (user_id);
CREATE INDEX IF NOT EXISTS idx_audit_org_id     ON data_plane_audit_log (org_id);
CREATE INDEX IF NOT EXISTS idx_audit_created_at ON data_plane_audit_log (created_at);
CREATE INDEX IF NOT EXISTS idx_audit_action     ON data_plane_audit_log (action);