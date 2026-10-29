-- Source kinds: 'drive' (the historical document-library delta sync) and
-- 'site_pages' (SharePoint site pages via Graph GET /sites/{id}/pages).
-- Site-pages sources have no drive, so drive_id loosens to nullable and the
-- drive-scope uniqueness becomes partial on kind = 'drive'. A site's pages
-- are registrable once per org, keyed on (organization_id, site_id).
ALTER TABLE sources ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'drive';
ALTER TABLE sources ALTER COLUMN drive_id DROP NOT NULL;

DROP INDEX IF EXISTS uq_sources_drive_scope;
CREATE UNIQUE INDEX IF NOT EXISTS uq_sources_drive_scope
    ON sources (organization_id, drive_id, COALESCE(folder_id, ''))
    WHERE kind = 'drive';
CREATE UNIQUE INDEX IF NOT EXISTS uq_sources_site_pages
    ON sources (organization_id, site_id)
    WHERE kind = 'site_pages';
