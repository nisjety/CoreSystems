-- 011_users_password_hash_avatar_not_null (down)
--
-- Revert to the pre-G37 schema: nullable columns with no default. We keep
-- the existing empty-string values in place because rolling back is just
-- about loosening the constraint, not rewriting data.

BEGIN;

ALTER TABLE users ALTER COLUMN password_hash DROP NOT NULL;
ALTER TABLE users ALTER COLUMN avatar        DROP NOT NULL;

ALTER TABLE users ALTER COLUMN password_hash DROP DEFAULT;
ALTER TABLE users ALTER COLUMN avatar        DROP DEFAULT;

COMMIT;
