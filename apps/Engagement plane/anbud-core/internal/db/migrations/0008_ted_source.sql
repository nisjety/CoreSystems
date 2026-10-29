-- 0008_ted_source.sql
-- A second source for the raw store: TED, the EU's journal of notices, read anonymously through its
-- search API (docs/SOURCES.md). Every evidence table and the day ledger named one source, doffin, in a
-- check; this migration admits ted as well, with the shape of its identifier.
--
-- The column keeps its name, doffin_id: renaming it would touch every query, and it is "the source's
-- own identifier" for both. A TED notice is stored under its publication number, the number in its
-- year, a dash and the year (741144-2026). The two shapes overlap (1234-2026 is both), so each check
-- ties the shape to the source of the row, and the identity (source, doffin_id, body_sha256) already
-- keeps a Doffin and a TED notice apart. raw_redactions has no source column: its check admits either
-- shape, and a tombstone names its row by raw_document_id in any case.
--
-- The checks are replaced under their own names. They are cheap to validate (no body is read), so
-- they are added valid. Forward-only; runs inside the caller's transaction (migrate.go, applyOne).

ALTER TABLE raw_documents DROP CONSTRAINT raw_documents_source_check;
ALTER TABLE raw_documents ADD CONSTRAINT raw_documents_source_check CHECK (source IN ('doffin', 'ted'));
ALTER TABLE raw_documents DROP CONSTRAINT raw_documents_doffin_id_check;
ALTER TABLE raw_documents ADD CONSTRAINT raw_documents_doffin_id_check CHECK (
    (source = 'doffin' AND doffin_id ~ '^[0-9]{4}-[0-9]{4,9}$')
    OR (source = 'ted' AND doffin_id ~ '^[0-9]{1,8}-[0-9]{4}$'));

ALTER TABLE download_attempts DROP CONSTRAINT download_attempts_source_check;
ALTER TABLE download_attempts ADD CONSTRAINT download_attempts_source_check CHECK (source IN ('doffin', 'ted'));
ALTER TABLE download_attempts DROP CONSTRAINT download_attempts_doffin_id_check;
ALTER TABLE download_attempts ADD CONSTRAINT download_attempts_doffin_id_check CHECK (
    (source = 'doffin' AND doffin_id ~ '^[0-9]{4}-[0-9]{4,9}$')
    OR (source = 'ted' AND doffin_id ~ '^[0-9]{1,8}-[0-9]{4}$'));

ALTER TABLE raw_search_pages DROP CONSTRAINT raw_search_pages_source_check;
ALTER TABLE raw_search_pages ADD CONSTRAINT raw_search_pages_source_check CHECK (source IN ('doffin', 'ted'));

ALTER TABLE sync_days DROP CONSTRAINT sync_days_source_check;
ALTER TABLE sync_days ADD CONSTRAINT sync_days_source_check CHECK (source IN ('doffin', 'ted'));

ALTER TABLE raw_redactions DROP CONSTRAINT raw_redactions_doffin_id_check;
ALTER TABLE raw_redactions ADD CONSTRAINT raw_redactions_doffin_id_check CHECK (
    doffin_id ~ '^[0-9]{4}-[0-9]{4,9}$' OR doffin_id ~ '^[0-9]{1,8}-[0-9]{4}$');

COMMENT ON COLUMN raw_documents.doffin_id IS 'The source''s own identifier of the notice: a Doffin id (2026-116564) when source is doffin, a TED publication number (741144-2026) when source is ted. The API shows a TED one as TED-741144-2026 (internal/noticeid).';
