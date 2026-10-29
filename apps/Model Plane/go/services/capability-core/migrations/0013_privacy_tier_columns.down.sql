-- Restore the gemini-1.5-pro row only if this migration's up.sql was the one
-- that soft-deleted it (deleted_at set).
UPDATE models
SET deleted_at = NULL, updated_at = now()
WHERE provider = 'google'
  AND name = 'gemini-1.5-pro'
  AND org_id IS NULL
  AND deleted_at IS NOT NULL;

ALTER TABLE models
    DROP CONSTRAINT IF EXISTS models_privacy_tier_check;

ALTER TABLE models
    DROP COLUMN IF EXISTS privacy_tier,
    DROP COLUMN IF EXISTS residency;
