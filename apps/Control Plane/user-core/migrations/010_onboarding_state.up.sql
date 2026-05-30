-- 010_onboarding_state — server-side onboarding state for G3 + G16.
--
-- Today's onboarding flow (velion's `onboarding-service.ts`) keeps the
-- multi-step wizard state in `localStorage`. That breaks two things:
--   1. Refresh on a different device loses progress (G3).
--   2. There is no server record of which step the user is on (G16) — the
--      existing `restoreStepFromServer()` reads from `/api/user/preferences`
--      which is fragile and only mirrors `onboarding_current_step` when the
--      client remembered to call `saveCurrentStep`.
--
-- Add two columns to `users` so velion can mirror state on every step
-- transition and on cancel/complete:
--
--   `onboarding_step`  — current wizard step name (free-form text; the
--                        client validates the enum). NULL when complete or
--                        never started. Indexed for "list users mid-wizard"
--                        queries.
--   `onboarding_state` — opaque JSONB blob carrying the partial step data
--                        the wizard collects (profile name, org choice,
--                        website URL, connector consents, …). Indexed only
--                        by user (PK already), no GIN index — query
--                        patterns are "fetch one row by user id," not
--                        "find users where state contains X."
--
-- Both nullable; existing rows get NULL on migration. `onboarding_complete`
-- (added in 004) stays as the authoritative "done?" flag — these two
-- columns describe in-flight state, not the terminal yes/no.

ALTER TABLE users
  ADD COLUMN IF NOT EXISTS onboarding_step  TEXT,
  ADD COLUMN IF NOT EXISTS onboarding_state JSONB;

CREATE INDEX IF NOT EXISTS idx_users_onboarding_step
  ON users(onboarding_step)
  WHERE onboarding_step IS NOT NULL;
