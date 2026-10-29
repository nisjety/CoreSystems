-- Browser Workspace Phase 3 continuation — profile name/scope metadata.
--
-- Promotes profile scope from an inferred gateway-side boolean
-- (`persistent_profile`) into first-class, queryable columns so the
-- profile-management UI (create/name/scope/inspect/delete) has a real
-- backing store instead of an opaque `profile_id`.
--
-- `scope` mirrors the SPA's `BrowserProfileScope` union
-- (`ephemeral | user_private | org_shared | run_scoped`), enforced with a
-- CHECK constraint so a bad write fails loudly instead of silently
-- degrading list/rename behavior.
--
-- Default/backfill value is `user_private`, NOT `ephemeral`. Every row
-- that can ever exist in this table — pre-existing or freshly inserted
-- by `ProfileStore::save()` without a prior `save_metadata()` call — was,
-- by construction, deliberately persisted: `quarry-edge`'s
-- create/update handlers reject `scope: ephemeral` outright (a truly
-- ephemeral session never gets a saved profile row at all, see
-- `quarry_browser::session::ProfileScope::Ephemeral`'s doc comment).
-- Backfilling/defaulting to `ephemeral` would write the one scope value
-- the API treats as a contradiction for a stored row straight into
-- storage. `user_private` mirrors the old pre-Phase-3 gateway inference
-- (`persistent_profile: true` -> `user_private`), which is exactly what
-- every one of these rows represents until a caller explicitly narrows
-- or widens the scope via `save_metadata()`.

ALTER TABLE quarry_profiles
    ADD COLUMN IF NOT EXISTS name  TEXT NULL,
    ADD COLUMN IF NOT EXISTS scope TEXT NOT NULL DEFAULT 'user_private';

ALTER TABLE quarry_profiles
    DROP CONSTRAINT IF EXISTS quarry_profiles_scope_check;

ALTER TABLE quarry_profiles
    ADD CONSTRAINT quarry_profiles_scope_check
    CHECK (scope IN ('ephemeral', 'user_private', 'org_shared', 'run_scoped'));
