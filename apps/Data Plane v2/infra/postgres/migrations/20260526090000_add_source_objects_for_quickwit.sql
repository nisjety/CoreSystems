-- Quickwit/Data Plane sparse-search read model support.
--
-- Postgres stays canonical. `source_objects` records connector inventory and
-- dedupe/hash metadata so Quickwit and other search stores can be rebuilt
-- from Postgres plus the outbox/event stream.

CREATE TABLE IF NOT EXISTS source_objects (
    source_object_id TEXT         PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id           TEXT         NOT NULL,
    connector        TEXT         NOT NULL,
    source           TEXT         NOT NULL,
    external_id      TEXT         NOT NULL,
    site_id          TEXT,
    drive_id         TEXT,
    item_id          TEXT,
    parent_id        TEXT,
    path             TEXT,
    name             TEXT         NOT NULL,
    mime_type        TEXT,
    size_bytes       BIGINT,
    etag             TEXT,
    ctag             TEXT,
    quickxor_hash    TEXT,
    sha1_hash        TEXT,
    content_hash     TEXT,
    acl_tags         TEXT[]       NOT NULL DEFAULT '{}',
    metadata         JSONB        NOT NULL DEFAULT '{}',
    modified_at      TIMESTAMPTZ,
    discovered_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    deleted_at       TIMESTAMPTZ,
    updated_at       TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, connector, external_id)
);

CREATE INDEX IF NOT EXISTS idx_source_objects_org_source
    ON source_objects (org_id, source);
CREATE INDEX IF NOT EXISTS idx_source_objects_drive_item
    ON source_objects (org_id, drive_id, item_id)
    WHERE drive_id IS NOT NULL AND item_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_source_objects_content_hash
    ON source_objects (org_id, content_hash)
    WHERE content_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_source_objects_quickxor_hash
    ON source_objects (org_id, quickxor_hash)
    WHERE quickxor_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_source_objects_sha1_hash
    ON source_objects (org_id, sha1_hash)
    WHERE sha1_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_source_objects_modified_at
    ON source_objects (org_id, modified_at DESC)
    WHERE modified_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_source_objects_metadata_gin
    ON source_objects USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_source_objects_acl_tags_gin
    ON source_objects USING GIN (acl_tags);

DO $$ BEGIN
    CREATE TRIGGER set_source_objects_updated_at BEFORE UPDATE ON source_objects
        FOR EACH ROW EXECUTE FUNCTION trigger_set_updated_at();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

COMMENT ON TABLE source_objects IS
    'Canonical connector source inventory. Quickwit indexes this as a rebuildable sparse-search read model.';
