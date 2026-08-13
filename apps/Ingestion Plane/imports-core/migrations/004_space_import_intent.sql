ALTER TABLE import_jobs
    ADD COLUMN IF NOT EXISTS space_import_intent JSONB;

CREATE INDEX IF NOT EXISTS idx_import_jobs_space_intent
    ON import_jobs ((space_import_intent->>'space_ref'))
    WHERE space_import_intent IS NOT NULL;
