DROP INDEX IF EXISTS billing_retry_jobs_processing_lease;

ALTER TABLE billing_usage_dedup
    DROP CONSTRAINT IF EXISTS billing_usage_dedup_event_id_bounded;

ALTER TABLE billing_usage_dedup
    DROP COLUMN IF EXISTS payload_hash;

DROP INDEX IF EXISTS billing_usage_events_event_id_unique;

ALTER TABLE billing_usage_events
    DROP CONSTRAINT IF EXISTS billing_usage_events_event_id_bounded;

ALTER TABLE billing_usage_events
    DROP COLUMN IF EXISTS event_id;
