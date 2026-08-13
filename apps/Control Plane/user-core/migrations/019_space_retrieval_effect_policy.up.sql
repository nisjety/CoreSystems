-- 019_space_retrieval_effect_policy — retrieval is a distinct effect class.
-- Existing organizations stay denied until the dedicated Control policy writer
-- explicitly enables it; a thread-create grant never implies data access.

BEGIN;

ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS retrieval_read_entitled BOOLEAN NOT NULL DEFAULT FALSE;

COMMIT;
