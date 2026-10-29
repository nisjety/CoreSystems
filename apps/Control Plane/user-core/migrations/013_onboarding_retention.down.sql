DROP INDEX IF EXISTS idx_users_incomplete_onboarding_expiry;

ALTER TABLE users
  DROP COLUMN IF EXISTS onboarding_completed_at,
  DROP COLUMN IF EXISTS onboarding_state_updated_at,
  DROP COLUMN IF EXISTS onboarding_expires_at,
  DROP COLUMN IF EXISTS onboarding_started_at;
