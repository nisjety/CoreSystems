ALTER TABLE import_jobs
  ADD COLUMN IF NOT EXISTS lease_owner TEXT,
  ADD COLUMN IF NOT EXISTS lease_expires_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS attempts INTEGER NOT NULL DEFAULT 0;

ALTER TABLE import_job_items
  ADD COLUMN IF NOT EXISTS document_payload JSONB;

CREATE INDEX IF NOT EXISTS idx_import_jobs_recovery
  ON import_jobs(status, lease_expires_at, created_at);
