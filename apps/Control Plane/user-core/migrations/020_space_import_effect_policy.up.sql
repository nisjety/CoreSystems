-- 020_space_import_effect_policy — imports are a separate durable-write
-- capability. Existing organizations stay denied until the dedicated Control
-- policy writer explicitly enables this flag.

BEGIN;

ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS import_write_entitled BOOLEAN NOT NULL DEFAULT FALSE;

COMMIT;
