-- WS14-S6: one room upload per (org, uploader, Space, Idempotency-Key). A replay
-- of the same key returns the existing job instead of creating a second one.
CREATE UNIQUE INDEX IF NOT EXISTS uq_import_jobs_space_upload_idem
    ON import_jobs (
        org_id,
        user_id,
        (space_import_intent->>'space_ref'),
        (space_import_intent->>'idempotency_key')
    )
    WHERE source_type = 'space_upload' AND space_import_intent IS NOT NULL;
