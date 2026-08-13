-- Recipient-audience revision is intentionally distinct from the aggregate
-- authority revision. A shared thread must retain the exact participant-set
-- revision that authorized each replayable thread/run record.

ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS recipient_audience_revision BIGINT,
    ADD COLUMN IF NOT EXISTS recipient_audience_hash TEXT;

ALTER TABLE runs
    ADD COLUMN IF NOT EXISTS recipient_audience_revision BIGINT,
    ADD COLUMN IF NOT EXISTS recipient_audience_hash TEXT;

ALTER TABLE threads
    ADD CONSTRAINT threads_recipient_audience_revision_complete_chk CHECK (
        (space_id IS NULL AND recipient_audience_revision IS NULL AND recipient_audience_hash IS NULL)
        OR
        (space_id IS NOT NULL AND recipient_audience_revision IS NOT NULL
            AND recipient_audience_revision > 0 AND recipient_audience_hash IS NOT NULL)
    ) NOT VALID;

ALTER TABLE runs
    ADD CONSTRAINT runs_recipient_audience_revision_complete_chk CHECK (
        (space_id IS NULL AND recipient_audience_revision IS NULL AND recipient_audience_hash IS NULL)
        OR
        (space_id IS NOT NULL AND recipient_audience_revision IS NOT NULL
            AND recipient_audience_revision > 0 AND recipient_audience_hash IS NOT NULL)
    ) NOT VALID;
