-- Each replayable transcript entry retains the non-secret Space/audience
-- snapshot inherited from its thread. This is provenance, not an authorization
-- substitute: append/resume effect-time reauthorization is introduced by the
-- shared-thread continuation slice.

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS space_id TEXT,
    ADD COLUMN IF NOT EXISTS recipient_audience_ref TEXT,
    ADD COLUMN IF NOT EXISTS recipient_audience_revision BIGINT,
    ADD COLUMN IF NOT EXISTS recipient_audience_hash TEXT,
    ADD COLUMN IF NOT EXISTS authority_revision BIGINT,
    ADD COLUMN IF NOT EXISTS resource_authorization_ref TEXT;

ALTER TABLE messages
    ADD CONSTRAINT messages_space_audience_context_complete_chk CHECK (
        (space_id IS NULL
            AND recipient_audience_ref IS NULL
            AND recipient_audience_revision IS NULL
            AND recipient_audience_hash IS NULL
            AND authority_revision IS NULL
            AND resource_authorization_ref IS NULL)
        OR
        (space_id IS NOT NULL
            AND recipient_audience_ref IS NOT NULL
            AND recipient_audience_revision IS NOT NULL AND recipient_audience_revision > 0
            AND recipient_audience_hash IS NOT NULL
            AND authority_revision IS NOT NULL AND authority_revision > 0
            AND resource_authorization_ref IS NOT NULL)
    ) NOT VALID;

CREATE INDEX IF NOT EXISTS idx_messages_space_audience
    ON messages (thread_id, recipient_audience_ref, recipient_audience_revision)
    WHERE space_id IS NOT NULL;
