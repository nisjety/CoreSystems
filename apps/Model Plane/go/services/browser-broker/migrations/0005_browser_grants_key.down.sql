-- Reverts 0005_browser_grants_key.up.sql. Key grants are deleted first: without
-- the key_id column they would read back as ordinary session grants of the
-- key's owner, i.e. a key grant would silently become a person's grant.
-- Idempotent, and a no-op when the table or the column does not exist.
DO $$
BEGIN
    IF to_regclass('browser_grants') IS NULL THEN
        RETURN;
    END IF;
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'browser_grants' AND column_name = 'key_id'
    ) THEN
        DELETE FROM browser_grants WHERE key_id IS NOT NULL;
    END IF;

    DROP INDEX IF EXISTS browser_grants_key_idx;
    ALTER TABLE browser_grants DROP CONSTRAINT IF EXISTS browser_grants_key_check;
    ALTER TABLE browser_grants DROP COLUMN IF EXISTS key_id;
END
$$;
