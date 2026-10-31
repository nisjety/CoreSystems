-- 0025_tool_policies_allow_needs_digest.up.sql
--
-- An allow in tool_policies is bound to the digest of the tool definition the person
-- reviewed (WS05-S5). 0017 writes that as
--   CHECK (decision <> 'allow' OR pinned_digest ~ '^[0-9a-f]{64}$')
-- and a CHECK passes when it evaluates to NULL. The regex test is NULL when pinned_digest
-- is NULL, so an allow with no digest at all was stored. The API never writes one: PUT
-- refuses an allow without a sha256 digest. This migration makes the table refuse it too.
--
-- Rows that already exist:
--   * A live allow with no digest is revoked, the way the API revokes a policy: revoked_at
--     and updated_at are set and the row is kept as history. Nothing is deleted, and the row
--     leaves tool_policies_active_key, so the unique index cannot clash. The engine never
--     honours an allow without a digest. For a tool an allow may relax it adds an ask, and
--     for any other tool it is ignored. Revoking it removes that ask and leaves the tool's
--     base decision, as with no policy set.
--   * A revoked allow with no digest stays as it is and is exempt from the check below.
--     Every read the API makes of policies filters on revoked_at IS NULL, and any write that
--     would make such a row live is checked again.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot. The revoke matches
-- nothing once no live unpinned allow is left, and the constraint is added only when it is
-- missing: Postgres has no ADD CONSTRAINT IF NOT EXISTS.

BEGIN;

UPDATE tool_policies
SET revoked_at = now(),
    updated_at = now()
WHERE decision = 'allow'
  AND pinned_digest IS NULL
  AND revoked_at IS NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'tool_policies'::regclass
          AND conname = 'tool_policies_allow_digest_present_check'
    ) THEN
        ALTER TABLE tool_policies
            ADD CONSTRAINT tool_policies_allow_digest_present_check
            CHECK (decision <> 'allow' OR pinned_digest IS NOT NULL OR revoked_at IS NOT NULL);
    END IF;
END;
$$;

COMMIT;
