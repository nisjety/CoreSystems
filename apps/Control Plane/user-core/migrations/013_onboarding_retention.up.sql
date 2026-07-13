-- Retain incomplete onboarding drafts for a fixed 30-day window. The canonical
-- identity remains after expiry; only the resumable draft is cleared.
ALTER TABLE users
  ADD COLUMN IF NOT EXISTS onboarding_started_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS onboarding_expires_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS onboarding_state_updated_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS onboarding_completed_at TIMESTAMPTZ;

UPDATE users
SET onboarding_started_at = COALESCE(onboarding_started_at, created_at),
    onboarding_expires_at = COALESCE(onboarding_expires_at, created_at + INTERVAL '30 days'),
    onboarding_state_updated_at = COALESCE(onboarding_state_updated_at, updated_at)
WHERE onboarding_complete = false
  AND (onboarding_step IS NOT NULL OR onboarding_state IS NOT NULL);

UPDATE users
SET onboarding_expires_at = NULL,
    onboarding_completed_at = COALESCE(onboarding_completed_at, updated_at),
    onboarding_step = NULL,
    onboarding_state = NULL,
    onboarding_state_updated_at = COALESCE(onboarding_state_updated_at, updated_at)
WHERE onboarding_complete = true;

CREATE INDEX IF NOT EXISTS idx_users_incomplete_onboarding_expiry
  ON users(onboarding_expires_at)
  WHERE onboarding_complete = false AND onboarding_expires_at IS NOT NULL;
