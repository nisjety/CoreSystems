-- Reconcile the original wiki schema with wiki-store-go's current repository
-- contract. This migration is forward-only and additive: legacy columns remain
-- available while current columns are backfilled from their legacy equivalents.

BEGIN;

ALTER TABLE wiki_pages ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ;

UPDATE wiki_pages
SET deleted_at = COALESCE(deleted_at, updated_at, NOW())
WHERE deleted_at IS NULL
  AND LOWER(COALESCE(page_status, '')) = 'deleted';

ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS org_id TEXT;
ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS source_type TEXT;
ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS source_ref TEXT;
ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS sync_status TEXT;
ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS details JSONB;
ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS original_chunks JSONB;
ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS processing_model TEXT;
ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS synthesis_prompt_hash TEXT;
ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS metadata JSONB;

UPDATE wiki_source_logs AS source_log
SET org_id = page.org_id
FROM wiki_pages AS page
WHERE source_log.page_id = page.page_id
  AND source_log.org_id IS DISTINCT FROM page.org_id;

UPDATE wiki_source_logs
SET source_type = COALESCE(NULLIF(source_type, ''), 'legacy'),
    source_ref = COALESCE(NULLIF(source_ref, ''), NULLIF(synthesis_prompt_hash, ''), 'legacy:' || log_id),
    sync_status = COALESCE(NULLIF(sync_status, ''), 'synced'),
    details = COALESCE(details, metadata, '{}'::JSONB);

ALTER TABLE wiki_source_logs ALTER COLUMN org_id SET NOT NULL;
ALTER TABLE wiki_source_logs ALTER COLUMN source_type SET NOT NULL;
ALTER TABLE wiki_source_logs ALTER COLUMN source_ref SET NOT NULL;
ALTER TABLE wiki_source_logs ALTER COLUMN sync_status SET DEFAULT 'synced';
ALTER TABLE wiki_source_logs ALTER COLUMN sync_status SET NOT NULL;
ALTER TABLE wiki_source_logs ALTER COLUMN details SET DEFAULT '{}'::JSONB;
ALTER TABLE wiki_source_logs ALTER COLUMN details SET NOT NULL;

ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS org_id TEXT;
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS action TEXT;
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS actor TEXT;
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS details JSONB;
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS kind TEXT;
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS detected_at TIMESTAMPTZ;
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS issue_type TEXT;
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS issue_details JSONB;
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS proposed_fix TEXT;
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS issue_status TEXT;
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS metadata JSONB;
ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS resolved_at TIMESTAMPTZ;

UPDATE wiki_maintenance_logs AS maintenance_log
SET org_id = page.org_id
FROM wiki_pages AS page
WHERE maintenance_log.page_id = page.page_id
  AND maintenance_log.org_id IS DISTINCT FROM page.org_id;

UPDATE wiki_maintenance_logs
SET action = COALESCE(NULLIF(action, ''), NULLIF(kind, ''), NULLIF(issue_type, ''), 'legacy'),
    actor = COALESCE(NULLIF(actor, ''), 'legacy'),
    details = COALESCE(details, issue_details, metadata, '{}'::JSONB),
    detected_at = COALESCE(detected_at, created_at, NOW());

UPDATE wiki_maintenance_logs
SET kind = action
WHERE kind IS NULL;

ALTER TABLE wiki_maintenance_logs ALTER COLUMN org_id SET NOT NULL;
ALTER TABLE wiki_maintenance_logs ALTER COLUMN action SET NOT NULL;
ALTER TABLE wiki_maintenance_logs ALTER COLUMN actor SET NOT NULL;
ALTER TABLE wiki_maintenance_logs ALTER COLUMN details SET DEFAULT '{}'::JSONB;
ALTER TABLE wiki_maintenance_logs ALTER COLUMN details SET NOT NULL;
ALTER TABLE wiki_maintenance_logs ALTER COLUMN detected_at SET DEFAULT NOW();
ALTER TABLE wiki_maintenance_logs ALTER COLUMN detected_at SET NOT NULL;

-- Keep both generations of writers/readers operational during rollback.
CREATE OR REPLACE FUNCTION sync_wiki_source_log_contract()
RETURNS TRIGGER AS $$
BEGIN
    SELECT page.org_id INTO NEW.org_id
    FROM wiki_pages AS page WHERE page.page_id = NEW.page_id;
    NEW.source_type := COALESCE(NULLIF(NEW.source_type, ''), 'legacy');
    NEW.source_ref := COALESCE(NULLIF(NEW.source_ref, ''), NULLIF(NEW.synthesis_prompt_hash, ''), 'legacy:' || NEW.log_id);
    NEW.sync_status := COALESCE(NULLIF(NEW.sync_status, ''), 'synced');
    NEW.details := COALESCE(NEW.details, NEW.metadata, '{}'::JSONB);
    NEW.synthesis_prompt_hash := COALESCE(NEW.synthesis_prompt_hash, NEW.source_ref);
    NEW.metadata := COALESCE(NEW.metadata, NEW.details);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DO $$ BEGIN
    CREATE TRIGGER sync_wiki_source_log_contract_before_write
    BEFORE INSERT OR UPDATE ON wiki_source_logs
    FOR EACH ROW EXECUTE FUNCTION sync_wiki_source_log_contract();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE OR REPLACE FUNCTION sync_wiki_maintenance_log_contract()
RETURNS TRIGGER AS $$
BEGIN
    SELECT page.org_id INTO NEW.org_id
    FROM wiki_pages AS page WHERE page.page_id = NEW.page_id;
    NEW.action := COALESCE(NULLIF(NEW.action, ''), NULLIF(NEW.kind, ''), NULLIF(NEW.issue_type, ''), 'legacy');
    NEW.actor := COALESCE(NULLIF(NEW.actor, ''), 'legacy');
    NEW.details := COALESCE(NEW.details, NEW.issue_details, NEW.metadata, '{}'::JSONB);
    NEW.kind := COALESCE(NEW.kind, NEW.action);
    NEW.issue_type := COALESCE(NEW.issue_type, NEW.action);
    NEW.issue_details := COALESCE(NEW.issue_details, NEW.details);
    NEW.metadata := COALESCE(NEW.metadata, NEW.details);
    NEW.detected_at := COALESCE(NEW.detected_at, NEW.created_at, NOW());
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DO $$ BEGIN
    CREATE TRIGGER sync_wiki_maintenance_log_contract_before_write
    BEFORE INSERT OR UPDATE ON wiki_maintenance_logs
    FOR EACH ROW EXECUTE FUNCTION sync_wiki_maintenance_log_contract();
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE INDEX IF NOT EXISTS idx_wp_org_not_deleted
    ON wiki_pages (org_id, updated_at DESC)
    WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_wsl_org_page_created
    ON wiki_source_logs (org_id, page_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_wml_org_page_created
    ON wiki_maintenance_logs (org_id, page_id, created_at DESC);

COMMIT;
