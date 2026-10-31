-- Reverts 0004_browser_grants_policy.up.sql. Human_browse and attach rows cannot
-- satisfy the 0001 constraints, so they are deleted first (an approval under an
-- attach grant goes with it: parent_grant_id is ON DELETE CASCADE). Idempotent,
-- and a no-op when the table does not exist.
DO $$
BEGIN
    IF to_regclass('browser_grants') IS NULL THEN
        RETURN;
    END IF;

    DELETE FROM browser_grants WHERE kind IN ('human_browse', 'attach');

    DROP INDEX IF EXISTS browser_grants_attach_idempotency_idx;
    DROP INDEX IF EXISTS browser_grants_owner_human_active_idx;

    ALTER TABLE browser_grants
        DROP CONSTRAINT IF EXISTS browser_grants_policy_mode_check,
        DROP CONSTRAINT IF EXISTS browser_grants_resource_check,
        DROP CONSTRAINT IF EXISTS browser_grants_kind_check,
        DROP CONSTRAINT IF EXISTS browser_grants_domains_check,
        DROP CONSTRAINT IF EXISTS browser_grants_parent_check;

    ALTER TABLE browser_grants
        ADD CONSTRAINT browser_grants_kind_check
            CHECK (kind IN ('session', 'run', 'sensitive_approval')),
        ADD CONSTRAINT browser_grants_domains_check
            CHECK (cardinality(allowed_domains) BETWEEN 1 AND 32),
        ADD CONSTRAINT browser_grants_parent_check
            CHECK ((kind = 'session') = (parent_grant_id IS NULL));

    ALTER TABLE browser_grants
        DROP COLUMN IF EXISTS policy_mode,
        DROP COLUMN IF EXISTS denied_domains,
        DROP COLUMN IF EXISTS resource_id;
END
$$;
