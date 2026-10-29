-- Content-free cross-plane Knowledge observability mirror. The local document
-- event must be published first; observability has an independent marker so an
-- Application Plane outage cannot delay durable Data Plane indexing.
ALTER TABLE documents_outbox
    ADD COLUMN IF NOT EXISTS observability_published BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS observability_published_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_documents_outbox_observability_pending
    ON documents_outbox (created_at)
    WHERE published = TRUE AND observability_published = FALSE;
