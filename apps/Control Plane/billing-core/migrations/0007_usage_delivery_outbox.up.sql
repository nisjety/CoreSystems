-- Make caller-stable usage identity visible on the aggregate row and bind the
-- idempotency reservation to the complete immutable payload. Historical rows
-- remain nullable; every new application write supplies event_id.
ALTER TABLE billing_usage_events
    ADD COLUMN IF NOT EXISTS event_id TEXT;

ALTER TABLE billing_usage_events
    ADD CONSTRAINT billing_usage_events_event_id_bounded
    CHECK (event_id IS NULL OR length(event_id) BETWEEN 1 AND 128) NOT VALID;

CREATE UNIQUE INDEX IF NOT EXISTS billing_usage_events_event_id_unique
    ON billing_usage_events (event_id)
    WHERE event_id IS NOT NULL;

ALTER TABLE billing_usage_dedup
    ADD COLUMN IF NOT EXISTS payload_hash TEXT;

-- NOT VALID preserves historical data while enforcing the boundary on every
-- new or updated row. The service applies the stricter character allow-list.
ALTER TABLE billing_usage_dedup
    ADD CONSTRAINT billing_usage_dedup_event_id_bounded
    CHECK (length(event_id) BETWEEN 1 AND 128) NOT VALID;

CREATE INDEX IF NOT EXISTS billing_retry_jobs_processing_lease
    ON billing_retry_jobs (updated_at)
    WHERE status = 'processing';
