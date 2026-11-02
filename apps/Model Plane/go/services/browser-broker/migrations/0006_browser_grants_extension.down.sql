-- Reverts 0006_browser_grants_extension.up.sql. Extension rows are deleted
-- first: without the kind they could not satisfy the restored constraint, and
-- an extension must never read back as some other grant. Idempotent, and a
-- no-op when the table does not exist.
DO $$
BEGIN
    IF to_regclass('browser_grants') IS NULL THEN
        RETURN;
    END IF;
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'browser_grants' AND column_name = 'consent_scope'
    ) THEN
        DELETE FROM browser_grants WHERE kind = 'origin_extension';
    END IF;

    DROP INDEX IF EXISTS browser_grants_extension_parent_idx;
    ALTER TABLE browser_grants DROP CONSTRAINT IF EXISTS browser_grants_extension_check;
    ALTER TABLE browser_grants DROP CONSTRAINT IF EXISTS browser_grants_kind_check;
    ALTER TABLE browser_grants ADD CONSTRAINT browser_grants_kind_check
        CHECK (kind IN ('session', 'human_browse', 'run', 'attach', 'sensitive_approval'));
    ALTER TABLE browser_grants DROP COLUMN IF EXISTS consent_scope;
END
$$;
