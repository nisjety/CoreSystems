-- Reverses 0136_approval_grant_use_token_refs.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only, inside
-- one transaction. Safe to run twice. After it, a standing-rule proof is again checked only
-- for its claims and lifetime, not for an earlier use.

BEGIN;

DROP INDEX IF EXISTS approvals_grant_use_token_ref_key;
ALTER TABLE approvals DROP COLUMN IF EXISTS grant_use_token_ref;

COMMIT;
