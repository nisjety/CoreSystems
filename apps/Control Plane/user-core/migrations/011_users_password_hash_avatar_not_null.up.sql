-- 011_users_password_hash_avatar_not_null — durable fix for G37.
--
-- Wave 9 (§8.23) hit a production cascade where social-signed-in users were
-- forced back into the onboarding wizard because pgx's `Scan` failed when
-- reading NULL into the Go struct's non-pointer `PasswordHash` / `Avatar`
-- string fields. `GetOrCreateUser` then treated *any* GetByID error as
-- "user doesn't exist", attempted INSERT, and hit a primary-key violation.
--
-- §8.23 applied a Band-Aid by wrapping the 8 SELECT/RETURNING queries in
-- `COALESCE(col, '')`. This migration makes that Band-Aid unnecessary by
-- enforcing the in-app contract at the schema level: both columns are
-- non-null with an empty-string default. Empty string is already the
-- sentinel `GetOrCreateUser` writes for OAuth users (see
-- `Repository.CreateWithID:77`), so this is a no-op for new rows.
--
-- Steps are ordered so the migration is safe under concurrent reads: the
-- backfill runs before the constraint, the constraint runs after the
-- default exists (so any in-flight INSERT that omits the columns picks up
-- the default instead of a NULL that the new NOT NULL would reject).

BEGIN;

-- Backfill: replace existing NULLs with the empty-string sentinel.
UPDATE users SET password_hash = '' WHERE password_hash IS NULL;
UPDATE users SET avatar        = '' WHERE avatar        IS NULL;

-- Establish the default so future INSERTs that omit these columns work.
ALTER TABLE users ALTER COLUMN password_hash SET DEFAULT '';
ALTER TABLE users ALTER COLUMN avatar        SET DEFAULT '';

-- Enforce NOT NULL now that the table is clean and a default exists.
ALTER TABLE users ALTER COLUMN password_hash SET NOT NULL;
ALTER TABLE users ALTER COLUMN avatar        SET NOT NULL;

COMMIT;
