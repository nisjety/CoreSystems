-- finspo-core initial schema
-- Owned by the finspo-core service; applied at startup via internal/db.ApplyMigrations.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- Generic updated_at trigger function (shared by all tables in this schema).
CREATE OR REPLACE FUNCTION finspo_set_updated_at()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;

-- ---------------------------------------------------------------------------
-- sources: a tenant + SharePoint site/drive triple. One row per drive we sync.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sources (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id    TEXT        NOT NULL,
    tenant_id          TEXT        NOT NULL,
    site_id            TEXT        NOT NULL,
    site_web_url       TEXT,
    drive_id           TEXT        NOT NULL,
    drive_name         TEXT,
    drive_type         TEXT,
    enabled            BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (organization_id, drive_id)
);

CREATE INDEX IF NOT EXISTS idx_sources_org ON sources (organization_id);
CREATE INDEX IF NOT EXISTS idx_sources_site ON sources (site_id);

DROP TRIGGER IF EXISTS trg_sources_updated_at ON sources;
CREATE TRIGGER trg_sources_updated_at
    BEFORE UPDATE ON sources
    FOR EACH ROW EXECUTE PROCEDURE finspo_set_updated_at();

-- ---------------------------------------------------------------------------
-- delta_cursors: per-drive Graph delta token + last-sync state.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS delta_cursors (
    source_id        UUID        PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
    delta_token      TEXT,
    delta_link       TEXT,
    last_synced_at   TIMESTAMPTZ,
    last_status      TEXT,
    last_error       TEXT,
    items_seen_total BIGINT      NOT NULL DEFAULT 0,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

DROP TRIGGER IF EXISTS trg_delta_cursors_updated_at ON delta_cursors;
CREATE TRIGGER trg_delta_cursors_updated_at
    BEFORE UPDATE ON delta_cursors
    FOR EACH ROW EXECUTE PROCEDURE finspo_set_updated_at();

-- ---------------------------------------------------------------------------
-- items: source-object state for every DriveItem the connector has observed.
-- Hashes captured from Graph (quickXorHash, sha1Hash); SharePoint does NOT
-- expose sha256Hash so the column is intentionally omitted.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS items (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id       UUID        NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    organization_id TEXT        NOT NULL,
    item_id         TEXT        NOT NULL,
    parent_item_id  TEXT,
    path            TEXT        NOT NULL,
    name            TEXT        NOT NULL,
    mime_type       TEXT,
    size_bytes      BIGINT,
    is_folder       BOOLEAN     NOT NULL DEFAULT FALSE,
    modified_at     TIMESTAMPTZ,
    etag            TEXT,
    ctag            TEXT,
    web_url         TEXT,
    quick_xor_hash  TEXT,
    sha1_hash       TEXT,
    deleted_at      TIMESTAMPTZ,
    raw             JSONB       NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (source_id, item_id)
);

CREATE INDEX IF NOT EXISTS idx_items_org              ON items (organization_id);
CREATE INDEX IF NOT EXISTS idx_items_source_parent    ON items (source_id, parent_item_id);
CREATE INDEX IF NOT EXISTS idx_items_modified         ON items (modified_at DESC);
CREATE INDEX IF NOT EXISTS idx_items_quick_xor_hash   ON items (quick_xor_hash) WHERE quick_xor_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_sha1_hash        ON items (sha1_hash)      WHERE sha1_hash      IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_size             ON items (size_bytes)     WHERE size_bytes     IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_items_deleted          ON items (deleted_at)     WHERE deleted_at     IS NOT NULL;

DROP TRIGGER IF EXISTS trg_items_updated_at ON items;
CREATE TRIGGER trg_items_updated_at
    BEFORE UPDATE ON items
    FOR EACH ROW EXECUTE PROCEDURE finspo_set_updated_at();

-- ---------------------------------------------------------------------------
-- permissions: normalized ACL summary per item. Used for governance reports
-- ONLY — at retrieval time, callers MUST re-verify access against Graph.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS permissions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    item_pk         UUID        NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    principal_id    TEXT,
    principal_type  TEXT,
    principal_name  TEXT,
    roles           TEXT[]      NOT NULL DEFAULT '{}',
    link_scope      TEXT,
    link_type       TEXT,
    inherited_from  TEXT,
    perm_hash       TEXT        NOT NULL,
    raw             JSONB       NOT NULL DEFAULT '{}'::jsonb,
    captured_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (item_pk, perm_hash)
);

CREATE INDEX IF NOT EXISTS idx_permissions_item      ON permissions (item_pk);
CREATE INDEX IF NOT EXISTS idx_permissions_principal ON permissions (principal_id) WHERE principal_id IS NOT NULL;

-- ---------------------------------------------------------------------------
-- audit_log: append-only record of governance actions (delete/archive
-- proposals, approvals, executions). Never auto-trimmed.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS audit_log (
    id              BIGSERIAL PRIMARY KEY,
    organization_id TEXT        NOT NULL,
    actor           TEXT        NOT NULL,
    action          TEXT        NOT NULL,
    target_kind     TEXT        NOT NULL,
    target_id       TEXT        NOT NULL,
    payload         JSONB       NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_log_org_created ON audit_log (organization_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_log_target      ON audit_log (target_kind, target_id);
