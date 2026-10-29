-- 0012_fts_source.sql
-- A fourth source for the raw store: Find a Tender, the UK's public procurement service, read anonymously
-- through its OCDS release package API (docs/SOURCES.md). Migrations 0002, 0008 and 0011 name the sources in
-- checks; this migration admits fts as well, with the shape of its identifier.
--
-- A Find a Tender notice is stored under its notice number: the number in its year, a dash and the year
-- (101800-2026). That is TED's shape, so each check ties the shape to the source of the row, as 0008 and 0011
-- do; the public id carries the FTS- prefix, and the identity (source, doffin_id, body_sha256) keeps the copies
-- apart. raw_redactions has no source column: its check admits any of the four shapes, and a tombstone names
-- its row by raw_document_id in any case.
--
-- The checks are replaced under their own names. They are cheap to validate (no body is read), so they are
-- added valid. Forward-only; runs inside the caller's transaction (migrate.go, applyOne).

ALTER TABLE raw_documents DROP CONSTRAINT raw_documents_source_check;
ALTER TABLE raw_documents ADD CONSTRAINT raw_documents_source_check CHECK (source IN ('doffin', 'ted', 'boamp', 'fts'));
ALTER TABLE raw_documents DROP CONSTRAINT raw_documents_doffin_id_check;
ALTER TABLE raw_documents ADD CONSTRAINT raw_documents_doffin_id_check CHECK (
    (source = 'doffin' AND doffin_id ~ '^[0-9]{4}-[0-9]{4,9}$')
    OR (source = 'ted' AND doffin_id ~ '^[0-9]{1,8}-[0-9]{4}$')
    OR (source = 'boamp' AND doffin_id ~ '^[0-9]{2}-[0-9]{4,9}$')
    OR (source = 'fts' AND doffin_id ~ '^[0-9]{1,8}-[0-9]{4}$'));

ALTER TABLE download_attempts DROP CONSTRAINT download_attempts_source_check;
ALTER TABLE download_attempts ADD CONSTRAINT download_attempts_source_check CHECK (source IN ('doffin', 'ted', 'boamp', 'fts'));
ALTER TABLE download_attempts DROP CONSTRAINT download_attempts_doffin_id_check;
ALTER TABLE download_attempts ADD CONSTRAINT download_attempts_doffin_id_check CHECK (
    (source = 'doffin' AND doffin_id ~ '^[0-9]{4}-[0-9]{4,9}$')
    OR (source = 'ted' AND doffin_id ~ '^[0-9]{1,8}-[0-9]{4}$')
    OR (source = 'boamp' AND doffin_id ~ '^[0-9]{2}-[0-9]{4,9}$')
    OR (source = 'fts' AND doffin_id ~ '^[0-9]{1,8}-[0-9]{4}$'));

ALTER TABLE raw_search_pages DROP CONSTRAINT raw_search_pages_source_check;
ALTER TABLE raw_search_pages ADD CONSTRAINT raw_search_pages_source_check CHECK (source IN ('doffin', 'ted', 'boamp', 'fts'));

ALTER TABLE sync_days DROP CONSTRAINT sync_days_source_check;
ALTER TABLE sync_days ADD CONSTRAINT sync_days_source_check CHECK (source IN ('doffin', 'ted', 'boamp', 'fts'));

ALTER TABLE raw_redactions DROP CONSTRAINT raw_redactions_doffin_id_check;
ALTER TABLE raw_redactions ADD CONSTRAINT raw_redactions_doffin_id_check CHECK (
    doffin_id ~ '^[0-9]{4}-[0-9]{4,9}$'
    OR doffin_id ~ '^[0-9]{1,8}-[0-9]{4}$'
    OR doffin_id ~ '^[0-9]{2}-[0-9]{4,9}$');

COMMENT ON COLUMN raw_documents.doffin_id IS 'The source''s own identifier of the notice: a Doffin id (2026-116564) when source is doffin, a TED publication number (741144-2026) when source is ted, a BOAMP identifier (26-104530) when source is boamp, a Find a Tender notice number (101800-2026) when source is fts. The API shows a TED one as TED-741144-2026, a BOAMP one as BOAMP-26-104530 and a Find a Tender one as FTS-101800-2026 (internal/noticeid).';
