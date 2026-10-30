-- 0011_boamp_source.sql
-- A third source for the raw store: BOAMP, the French official bulletin of public tenders, read anonymously
-- through its open-data records API (docs/SOURCES.md). Migrations 0002 and 0008 name the sources in checks;
-- this migration admits boamp as well, with the shape of its identifier.
--
-- A BOAMP notice is stored under its identifier: the two digits of its year, a dash and the number of the
-- notice in that year (26-104530). That shape overlaps TED's (26-1234 is both), so each check ties the shape
-- to the source of the row, as 0008 does, and the identity (source, doffin_id, body_sha256) keeps the copies
-- apart. raw_redactions has no source column: its check admits any of the three shapes, and a tombstone names
-- its row by raw_document_id in any case.
--
-- The checks are replaced under their own names. They are cheap to validate (no body is read), so they are
-- added valid. Forward-only; runs inside the caller's transaction (migrate.go, applyOne).

ALTER TABLE raw_documents DROP CONSTRAINT raw_documents_source_check;
ALTER TABLE raw_documents ADD CONSTRAINT raw_documents_source_check CHECK (source IN ('doffin', 'ted', 'boamp'));
ALTER TABLE raw_documents DROP CONSTRAINT raw_documents_doffin_id_check;
ALTER TABLE raw_documents ADD CONSTRAINT raw_documents_doffin_id_check CHECK (
    (source = 'doffin' AND doffin_id ~ '^[0-9]{4}-[0-9]{4,9}$')
    OR (source = 'ted' AND doffin_id ~ '^[0-9]{1,8}-[0-9]{4}$')
    OR (source = 'boamp' AND doffin_id ~ '^[0-9]{2}-[0-9]{4,9}$'));

ALTER TABLE download_attempts DROP CONSTRAINT download_attempts_source_check;
ALTER TABLE download_attempts ADD CONSTRAINT download_attempts_source_check CHECK (source IN ('doffin', 'ted', 'boamp'));
ALTER TABLE download_attempts DROP CONSTRAINT download_attempts_doffin_id_check;
ALTER TABLE download_attempts ADD CONSTRAINT download_attempts_doffin_id_check CHECK (
    (source = 'doffin' AND doffin_id ~ '^[0-9]{4}-[0-9]{4,9}$')
    OR (source = 'ted' AND doffin_id ~ '^[0-9]{1,8}-[0-9]{4}$')
    OR (source = 'boamp' AND doffin_id ~ '^[0-9]{2}-[0-9]{4,9}$'));

ALTER TABLE raw_search_pages DROP CONSTRAINT raw_search_pages_source_check;
ALTER TABLE raw_search_pages ADD CONSTRAINT raw_search_pages_source_check CHECK (source IN ('doffin', 'ted', 'boamp'));

ALTER TABLE sync_days DROP CONSTRAINT sync_days_source_check;
ALTER TABLE sync_days ADD CONSTRAINT sync_days_source_check CHECK (source IN ('doffin', 'ted', 'boamp'));

ALTER TABLE raw_redactions DROP CONSTRAINT raw_redactions_doffin_id_check;
ALTER TABLE raw_redactions ADD CONSTRAINT raw_redactions_doffin_id_check CHECK (
    doffin_id ~ '^[0-9]{4}-[0-9]{4,9}$'
    OR doffin_id ~ '^[0-9]{1,8}-[0-9]{4}$'
    OR doffin_id ~ '^[0-9]{2}-[0-9]{4,9}$');

COMMENT ON COLUMN raw_documents.doffin_id IS 'The source''s own identifier of the notice: a Doffin id (2026-116564) when source is doffin, a TED publication number (741144-2026) when source is ted, a BOAMP identifier (26-104530) when source is boamp. The API shows a TED one as TED-741144-2026 and a BOAMP one as BOAMP-26-104530 (internal/noticeid).';
