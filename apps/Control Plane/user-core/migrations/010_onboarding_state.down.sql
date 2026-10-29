-- Rollback for 010_onboarding_state. Drops the two new columns + the
-- step index. `onboarding_complete` (from 004) is untouched.

DROP INDEX IF EXISTS idx_users_onboarding_step;

ALTER TABLE users
  DROP COLUMN IF EXISTS onboarding_state,
  DROP COLUMN IF EXISTS onboarding_step;
