-- Per-User Data Ownership & Sharing (PR-2): add owner_id + visibility to
-- documents and grandfather existing rows as org-shared — visible to the whole
-- org, owned by their creator. NEW rows also default to 'org'; Private is an
-- explicit, audited opt-in once the honesty gate is live (PR-4/PR-6). This is
-- non-breaking: no existing row becomes inaccessible to any teammate.
--
-- Deployment ordering: requires user-core migration 012 (resource_grants) for
-- the share path, but this migration is independent of it at the SQL level.
--
-- The migrator (tools/migrator) wraps every file in a transaction, so this file
-- must NOT declare its own BEGIN/COMMIT (a nested COMMIT would close the
-- migrator's tx before it records the version). For the same reason the owner
-- index is built non-concurrently — CREATE INDEX CONCURRENTLY cannot run inside
-- a transaction. On a large live documents table, run this migration in a
-- maintenance window (the ALTER ... SET NOT NULL scan + index build take an
-- AccessExclusiveLock); at current scale the cost is negligible.

ALTER TABLE documents ADD COLUMN IF NOT EXISTS owner_id   TEXT;
ALTER TABLE documents ADD COLUMN IF NOT EXISTS visibility TEXT NOT NULL DEFAULT 'org';

-- visibility domain guard (explicit name so it is idempotent across init.sql + this migration).
ALTER TABLE documents DROP CONSTRAINT IF EXISTS documents_visibility_chk;
ALTER TABLE documents ADD  CONSTRAINT documents_visibility_chk CHECK (visibility IN ('private', 'org', 'shared'));

-- Grandfather: owner = creator, system account when the creator is unknown.
UPDATE documents SET owner_id = COALESCE(created_by, 'org-system-account') WHERE owner_id IS NULL;

-- Future inserts that omit owner_id (e.g. the retrieval-engine gRPC create path
-- until PR-3 threads identity) fall back to the system account, never NULL.
ALTER TABLE documents ALTER COLUMN owner_id SET DEFAULT 'org-system-account';
ALTER TABLE documents ALTER COLUMN owner_id SET NOT NULL;

-- "Owned by me" + owner-scoped retrieval/list filters.
CREATE INDEX IF NOT EXISTS idx_documents_owner ON documents (org_id, owner_id);

-- Post-condition: prove the grandfather backfill left zero NULL owner_id rows.
DO $$
DECLARE n bigint;
BEGIN
    SELECT count(*) INTO n FROM documents WHERE owner_id IS NULL;
    IF n > 0 THEN
        RAISE EXCEPTION 'ownership backfill left % NULL owner_id rows', n;
    END IF;
END $$;
